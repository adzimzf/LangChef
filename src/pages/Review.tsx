import { useEffect, useMemo, useState, useRef, useCallback } from 'react'
import { invoke } from '@tauri-apps/api/core'
import type { Grade, CardState } from '../lib/srs'
import {
  scheduleCard,
  previewGrades,
  formatNextReview,
  difficultyColor,
  difficultyBgClass,
  stateLabel,
  DEFAULT_SRS_CONFIG,
} from '../lib/srs'
import {
  ImportOutlined,
  ReloadOutlined,
  RobotOutlined,
  EditOutlined,
  CheckOutlined,
  CloseOutlined,
  SettingOutlined,
} from '@ant-design/icons'

type ReviewItem = {
  id: string
  content: string
  EF: number
  I: number
  R: number
  state: string
  step: number
  lapse_count: number
  next_review: number | null
  tags: string | null
  language: string | null
  detail: string | null
}

const LANGUAGES: Record<string, { label: string; flag: string }> = {
  ja: { label: 'Japanese', flag: '\u{1F1EF}\u{1F1F5}' },
  id: { label: 'Indonesian', flag: '\u{1F1EE}\u{1F1E9}' },
}

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

function todayKey(): string {
  return new Date().toISOString().slice(0, 10)
}

function getReviewedToday(): number {
  const stored = localStorage.getItem(`reviewed_${todayKey()}`)
  return stored ? parseInt(stored, 10) : 0
}

function incrementReviewedToday() {
  const key = `reviewed_${todayKey()}`
  const current = getReviewedToday()
  localStorage.setItem(key, String(current + 1))
}

function getNewCardsToday(): number {
  const stored = localStorage.getItem(`new_seen_${todayKey()}`)
  return stored ? parseInt(stored, 10) : 0
}

function incrementNewCardsToday() {
  const key = `new_seen_${todayKey()}`
  const current = getNewCardsToday()
  localStorage.setItem(key, String(current + 1))
}

// Grade button config
type GradeOption = { grade: Grade; label: string; nextLabel: string; color: string }

function parseDetailJson(detail: string): { reading?: string; meaning?: string; part_of_speech?: string; example?: string; example_meaning?: string } | null {
  try {
    const match = detail.match(/\{[\s\S]*\}/)
    if (!match) return null
    return JSON.parse(match[0])
  } catch {
    return null
  }
}

/**
 * Convert a ReviewItem (from DB) into the Card shape expected by srs.ts
 */
function itemToCard(item: ReviewItem) {
  return {
    id: item.id,
    content: item.content,
    EF: item.EF,
    I: item.I,
    R: item.R,
    state: (item.state || 'new') as CardState,
    step: item.step || 0,
    lapseCount: item.lapse_count || 0,
    lastReviewed: undefined,
    nextReview: item.next_review ? item.next_review * 1000 : undefined, // DB stores seconds, Card expects ms
    history: [],
    tags: item.tags ? item.tags.split(',') : undefined,
    language: item.language || undefined,
    detail: item.detail || undefined,
  }
}

export default function Review() {
  const [allItems, setAllItems] = useState<ReviewItem[]>([])
  const [selectedLang, setSelectedLang] = useState<string | null>(null)
  const [revealed, setRevealed] = useState(false)
  const [generatingDetail, setGeneratingDetail] = useState(false)
  const [editingDetail, setEditingDetail] = useState<string | null>(null)
  const [editDetailText, setEditDetailText] = useState('')
  const [showImport, setShowImport] = useState(false)
  const [importText, setImportText] = useState('')
  const [importLang, setImportLang] = useState('ja')
  const [importing, setImporting] = useState(false)
  const [importResult, setImportResult] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [reviewedToday, setReviewedToday] = useState(getReviewedToday())
  const [newCardsToday, setNewCardsToday] = useState(getNewCardsToday())
  const fileInputRef = useRef<HTMLInputElement>(null)
  const maxReviews = getMaxReviews()
  const maxNewCards = getMaxNewCards()

  // Session queue: ordered list of cards to review.
  // Lapsed cards go to the back; passed cards are removed.
  // Card state is updated locally after each grade so that
  // subsequent grade previews are accurate without needing a DB reload.
  const [sessionQueue, setSessionQueue] = useState<ReviewItem[]>([])
  const [sessionCompleted, setSessionCompleted] = useState(0)

  // Track if this is the initial load (to build queue from DB)
  const initialLoadDone = useRef(false)

  // Tick counter to force re-evaluation of which cards are due.
  // Learning/relearning cards have short step intervals (e.g. 1m, 10m) and
  // become due when their next_review timestamp passes. Without ticking,
  // they'd remain invisible until the user takes some other action.
  const [tick, setTick] = useState(0)
  useEffect(() => {
    const interval = setInterval(() => setTick(t => t + 1), 5000)
    return () => clearInterval(interval)
  }, [])

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

  // Group items by language — skip items without a language
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

  // Auto-select first language if none selected
  useEffect(() => {
    const langs = Object.keys(languageGroups)
    if (langs.length > 0 && !selectedLang) {
      setSelectedLang(langs[0])
    }
  }, [languageGroups])

  // Build session queue from DB. Only called on mount, language change, or manual refresh.
  const buildQueue = useCallback((lang: string | null) => {
    if (!lang) {
      setSessionQueue([])
      return
    }
    const items: ReviewItem[] = languageGroups[lang] || []
    const now = Date.now() / 1000
    const newCardsSeen = getNewCardsToday()
    // Cards are due if: no next_review, or next_review <= now
    // Also include new/learning/relearning cards (they may have short-term next_review already passed)
    // But limit the number of new (state='new') cards introduced today
    let newCardsInQueue = 0
    const due = items.filter((item: ReviewItem) => {
      // Skip cards that aren't due yet
      if (item.next_review && item.next_review > now) return false
      // For new cards, enforce the daily new card limit
      if (item.state === 'new' || !item.state) {
        if (newCardsSeen + newCardsInQueue >= maxNewCards) return false
        newCardsInQueue++
      }
      return true
    })
    setSessionQueue(due)
    setSessionCompleted(0)
  }, [languageGroups, maxNewCards])

  // Build queue on initial load
  useEffect(() => {
    if (selectedLang && !loading && !initialLoadDone.current) {
      buildQueue(selectedLang)
      initialLoadDone.current = true
    }
  }, [selectedLang, loading, buildQueue])

  // Build queue on language change
  const prevLang = useRef<string | null>(null)
  useEffect(() => {
    if (selectedLang && selectedLang !== prevLang.current && initialLoadDone.current) {
      buildQueue(selectedLang)
      setRevealed(false)
    }
    prevLang.current = selectedLang
  }, [selectedLang, buildQueue])

  const currentCard = sessionQueue[0]
  const dailyLimitReached = reviewedToday >= maxReviews

  // Find the first immediately-due card from the queue.
  // Cards in learning/relearning may have a short-term next_review that hasn't
  // arrived yet — those should be skipped until their step timer elapses.
  const nowSeconds = Date.now() / 1000
  const dueQueueIndex = sessionQueue.findIndex(
    item => !item.next_review || item.next_review <= nowSeconds
  )
  const visibleCard = dueQueueIndex >= 0 ? sessionQueue[dueQueueIndex] : null

  // Count how many cards in the queue are immediately due right now
  const immediatelyDueCount = sessionQueue.filter(
    item => !item.next_review || item.next_review <= nowSeconds
  ).length

  // Stats for the selected language
  const deckItems: ReviewItem[] = selectedLang ? (languageGroups[selectedLang] || []) : []
  const dueInDeck = (() => {
    if (!selectedLang) return 0
    const now = Date.now() / 1000
    return deckItems.filter((item: ReviewItem) => !item.next_review || item.next_review <= now).length
  })()

  // Anki-style grade options with dynamic next-review preview
  const gradeOptions: GradeOption[] = (() => {
    if (!visibleCard) return []
    const card = itemToCard(visibleCard)
    const previews = previewGrades(card, DEFAULT_SRS_CONFIG)
    return [
      { grade: 'again', label: 'Again', nextLabel: formatNextReview(previews.again), color: 'bg-red-500' },
      { grade: 'hard', label: 'Hard', nextLabel: formatNextReview(previews.hard), color: 'bg-orange-500' },
      { grade: 'good', label: 'Good', nextLabel: formatNextReview(previews.good), color: 'bg-blue-500' },
      { grade: 'easy', label: 'Easy', nextLabel: formatNextReview(previews.easy), color: 'bg-green-500' },
    ]
  })()

  async function handleGrade(grade: Grade) {
    if (!visibleCard) return

    // Compute new SRS state locally for immediate UI update
    const card = itemToCard(visibleCard)
    const { card: updated, lapsed } = scheduleCard(card, grade, DEFAULT_SRS_CONFIG)

    // Update SRS in DB (fire and forget)
    invoke('db_update_srs', { id: visibleCard.id, grade }).catch(() => {})

    incrementReviewedToday()
    setReviewedToday(prev => prev + 1)

    // Track new cards introduced today
    if (card.state === 'new') {
      incrementNewCardsToday()
      setNewCardsToday(prev => prev + 1)
    }

    setRevealed(false)

    // Determine if the card graduated (left learning/relearning into review)
    const graduated = (updated.state === 'review') && (card.state === 'new' || card.state === 'learning' || card.state === 'relearning')

    // Update session queue with locally computed new state
    setSessionQueue(prev => {
      // Find the card in the queue (it might not be at [0] if we skipped waiting cards)
      const idx = prev.findIndex(item => item.id === visibleCard.id)
      if (idx < 0) return prev

      const before = prev.slice(0, idx)
      const after = prev.slice(idx + 1)

      // Map the updated card back to ReviewItem shape
      const updatedItem: ReviewItem = {
        ...prev[idx],
        EF: updated.EF,
        I: updated.I,
        R: updated.R,
        state: updated.state,
        step: updated.step,
        lapse_count: updated.lapseCount,
        next_review: updated.nextReview ? Math.round(updated.nextReview / 1000) : null,
      }

      let newQueue: ReviewItem[]
      if (graduated && !lapsed) {
        // Card graduated to Review: remove from session queue entirely.
        // It will reappear on a future day when its next_review arrives.
        newQueue = [...before, ...after]
      } else if (lapsed) {
        // Lapse: move updated card to the back of the queue (Anki behavior)
        newQueue = [...before, ...after, updatedItem]
      } else {
        // Still in learning/relearning: keep in queue with updated next_review.
        // It will become visible again once the step timer elapses.
        newQueue = [...before, updatedItem, ...after]
      }

      // If new card limit reached, remove remaining new cards from the queue
      const newSeenAfterGrade = card.state === 'new' ? newCardsToday + 1 : newCardsToday
      if (newSeenAfterGrade >= maxNewCards) {
        newQueue = newQueue.filter(item => item.state !== 'new' && item.state !== null)
      }

      return newQueue
    })

    if (graduated && !lapsed) {
      setSessionCompleted(prev => prev + 1)
    }
  }

  async function generateDetail() {
    if (!visibleCard) return
    const apiHost = localStorage.getItem('api_host') || ''
    const apiKey = localStorage.getItem('api_key') || ''
    const model = localStorage.getItem('api_model') || 'gpt-3.5-turbo'
    if (!apiHost || !apiKey) {
      alert('Please configure your API settings first (Settings page).')
      return
    }
    setGeneratingDetail(true)
    try {
      const lang = visibleCard.language || 'ja'
      const result = await invoke<string>('db_generate_detail', {
        apiHost, apiKey, model,
        content: visibleCard.content,
        language: lang,
      })
      setSessionQueue(prev => prev.map(item =>
        item.id === visibleCard.id ? { ...item, detail: result } : item
      ))
    } catch (e) {
      alert(`Failed to generate: ${e}`)
    }
    setGeneratingDetail(false)
  }

  async function saveDetail() {
    if (!editingDetail) return
    try {
      await invoke('db_update_review_detail', { id: editingDetail, detail: editDetailText })
      setSessionQueue(prev => prev.map(item =>
        item.id === editingDetail ? { ...item, detail: editDetailText } : item
      ))
    } catch {}
    setEditingDetail(null)
    setEditDetailText('')
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
          tags: undefined as string | undefined,
        }
      })
      const result = await invoke<string>('db_import_review_items', { items })
      setImportResult(result)
      // Reload DB and rebuild queue
      await loadItems()
      buildQueue(selectedLang)
    } catch (e) {
      setImportResult(`Error: ${e}`)
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
    // Need to wait for allItems to update, then rebuild
    // We do this by using a flag that triggers in the useEffect
    initialLoadDone.current = false
  }

  function renderDetailContent(detail: string | null) {
    if (!detail) {
      return (
        <div className="mt-4 p-3 bg-gray-50 rounded-xl text-center">
          <p className="text-sm text-gray-400 mb-3">No detail available for this card.</p>
          <div className="flex gap-2 justify-center">
            <button
              onClick={() => generateDetail()}
              disabled={generatingDetail}
              className="flex items-center gap-1.5 px-3 py-2 rounded-xl bg-blue-500 text-white text-sm font-medium hover:bg-blue-600 active:bg-blue-700 transition-colors disabled:opacity-50"
            >
              <RobotOutlined />
              {generatingDetail ? 'Generating...' : 'Generate with AI'}
            </button>
            <button
              onClick={() => {
                setEditingDetail(visibleCard?.id || null)
                setEditDetailText('')
              }}
              className="flex items-center gap-1.5 px-3 py-2 rounded-xl bg-gray-200 text-gray-700 text-sm font-medium hover:bg-gray-300 active:bg-gray-400 transition-colors"
            >
              <EditOutlined />
              Add manually
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
              <span className="text-xs text-gray-400 w-24 shrink-0">Reading</span>
              <span className="text-sm font-medium">{parsed.reading}</span>
            </div>
          )}
          {parsed.meaning && (
            <div className="flex items-baseline gap-2">
              <span className="text-xs text-gray-400 w-24 shrink-0">Meaning</span>
              <span className="text-sm font-medium">{parsed.meaning}</span>
            </div>
          )}
          {parsed.part_of_speech && (
            <div className="flex items-baseline gap-2">
              <span className="text-xs text-gray-400 w-24 shrink-0">Part of speech</span>
              <span className="text-xs px-2 py-0.5 rounded-full bg-gray-100 text-gray-600">{parsed.part_of_speech}</span>
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
          <button
            onClick={() => {
              setEditingDetail(visibleCard?.id || null)
              setEditDetailText(detail)
            }}
            className="text-xs text-gray-400 hover:text-gray-600 mt-1"
          >
            <EditOutlined className="mr-1" />Edit detail
          </button>
        </div>
      )
    }

    return (
      <div className="mt-4 text-left">
        <p className="text-sm text-gray-600 whitespace-pre-wrap">{detail}</p>
        <button
          onClick={() => {
            setEditingDetail(visibleCard?.id || null)
            setEditDetailText(detail)
          }}
          className="text-xs text-gray-400 hover:text-gray-600 mt-2"
        >
          <EditOutlined className="mr-1" />Edit detail
        </button>
      </div>
    )
  }

  // Edit detail modal
  if (editingDetail) {
    const card = sessionQueue.find(i => i.id === editingDetail) || allItems.find(i => i.id === editingDetail)
    return (
      <div className="flex flex-col h-full -m-4 p-4">
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-lg font-semibold">Edit Detail</h2>
          <button
            onClick={() => { setEditingDetail(null); setEditDetailText('') }}
            className="w-8 h-8 flex items-center justify-center rounded-full hover:bg-gray-100"
          >
            <CloseOutlined />
          </button>
        </div>
        <div className="mb-2 text-xl font-semibold text-center">{card?.content}</div>
        <textarea
          value={editDetailText}
          onChange={(e) => setEditDetailText(e.target.value)}
          placeholder='Enter detail as JSON: {"reading":"...","meaning":"...","part_of_speech":"...","example":"...","example_meaning":"..."}'
          className="flex-1 w-full p-3 bg-gray-50 rounded-xl border-none outline-none text-sm resize-none"
        />
        <button
          onClick={saveDetail}
          className="mt-3 w-full py-3 rounded-xl bg-blue-500 text-white font-medium hover:bg-blue-600 active:bg-blue-700 transition-colors flex items-center justify-center gap-2"
        >
          <CheckOutlined /> Save
        </button>
      </div>
    )
  }

  // Import modal
  if (showImport) {
    return (
      <div className="flex flex-col h-full -m-4 p-4">
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-lg font-semibold">Import Cards</h2>
          <button
            onClick={() => { setShowImport(false); setImportResult(null) }}
            className="w-8 h-8 flex items-center justify-center rounded-full hover:bg-gray-100"
          >
            <CloseOutlined />
          </button>
        </div>

        <div className="mb-3">
          <label className="text-sm text-gray-500 mb-1 block">Language</label>
          <div className="flex gap-2">
            {Object.entries(LANGUAGES).map(([code, lang]) => (
              <button
                key={code}
                onClick={() => setImportLang(code)}
                className={`px-3 py-1.5 rounded-xl text-sm font-medium transition-colors ${
                  importLang === code
                    ? 'bg-blue-500 text-white'
                    : 'bg-gray-100 text-gray-600 hover:bg-gray-200'
                }`}
              >
                {lang.flag} {lang.label}
              </button>
            ))}
          </div>
        </div>

        <div className="mb-2">
          <p className="text-xs text-gray-400 mb-1">
            One word per line. Format: word,detail (detail is optional). Or load a CSV file.
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
            <ImportOutlined /> Load file
          </button>
        </div>

        <textarea
          value={importText}
          onChange={(e) => setImportText(e.target.value)}
          placeholder={"食べる,to eat\n飲む,to drink\n散歩,walk/stroll"}
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
            Cancel
          </button>
          <button
            onClick={handleImport}
            disabled={!importText.trim() || importing}
            className="flex-1 py-3 rounded-xl bg-blue-500 text-white font-medium hover:bg-blue-600 active:bg-blue-700 transition-colors disabled:opacity-50 flex items-center justify-center gap-2"
          >
            <ImportOutlined />
            {importing ? 'Importing...' : 'Import'}
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
        <p className="text-sm">Loading cards...</p>
      </div>
    )
  }

  // No cards at all
  if (allItems.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center py-16 text-gray-400 gap-4">
        <div className="text-5xl">📚</div>
        <h3 className="text-lg font-medium text-gray-600">No cards yet</h3>
        <p className="text-sm text-center max-w-xs">
          Flag unknown words in chat, or import cards to get started.
        </p>
        <button
          onClick={() => setShowImport(true)}
          className="flex items-center gap-1.5 px-4 py-2 rounded-full bg-blue-500 text-white text-sm font-medium hover:bg-blue-600 active:bg-blue-700 transition-colors"
        >
          <ImportOutlined /> Import Cards
        </button>
      </div>
    )
  }

  // Language tabs
  const langKeys = Object.keys(languageGroups)

  // Daily limit reached
  if (dailyLimitReached) {
    return (
      <div className="flex flex-col items-center justify-center py-16 text-gray-400 gap-4">
        <div className="text-5xl">🏆</div>
        <h3 className="text-lg font-medium text-gray-600">Daily limit reached!</h3>
        <p className="text-sm text-center max-w-xs">
          You've reviewed {reviewedToday} cards today ({newCardsToday} new). Come back tomorrow to keep learning!
        </p>
        <p className="text-xs text-gray-300">Limit: {maxReviews} reviews/day · {maxNewCards} new cards/day</p>
        <button
          onClick={loadItems}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-xl bg-gray-100 text-gray-600 text-sm font-medium hover:bg-gray-200 transition-colors"
        >
          <SettingOutlined /> Change in Settings
        </button>
      </div>
    )
  }

  return (
    <div className="flex flex-col h-full -m-4 p-4 overflow-y-auto">
      {/* Header with language tabs */}
      <div className="mb-3 shrink-0 sticky top-0 z-10 bg-gray-50 pt-4 pb-2">
        <div className="flex items-center justify-between mb-2">
          <h2 className="text-lg font-semibold">Review</h2>
          <div className="flex items-center gap-2">
            <span className="text-xs text-gray-400">{reviewedToday}/{maxReviews} today · {newCardsToday}/{maxNewCards} new</span>
            <button
              onClick={() => setShowImport(true)}
              className="flex items-center gap-1 px-2.5 py-1 rounded-lg bg-gray-100 text-gray-600 text-xs font-medium hover:bg-gray-200 transition-colors"
            >
              <ImportOutlined /> Import
            </button>
          </div>
        </div>

        {/* Language tabs */}
        {langKeys.length > 1 && (
          <div className="flex gap-1.5 mb-3">
            {langKeys.map(lang => {
              const info = LANGUAGES[lang]
              const count = (languageGroups[lang] || []).length
              const dueCount = (languageGroups[lang] || []).filter(
                (item: ReviewItem) => !item.next_review || item.next_review <= Date.now() / 1000
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
                  {info ? `${info.flag} ` : ''}{info?.label || lang}
                  <span className="ml-1 text-xs opacity-70">({dueCount}/{count})</span>
                </button>
              )
            })}
          </div>
        )}

        {/* Session progress */}
        {sessionQueue.length > 0 && (
          <div className="flex items-center gap-2">
            <div className="flex-1 h-1.5 bg-gray-100 rounded-full overflow-hidden">
              <div
                className="h-full bg-blue-500 rounded-full transition-all duration-300"
                style={{ width: `${(sessionCompleted + immediatelyDueCount) > 0 ? (sessionCompleted / (sessionCompleted + immediatelyDueCount)) * 100 : 0}%` }}
              />
            </div>
            <span className="text-xs text-gray-400 shrink-0">
              {immediatelyDueCount} due{sessionQueue.length - immediatelyDueCount > 0 ? ` (${sessionQueue.length - immediatelyDueCount} waiting)` : ''}
            </span>
          </div>
        )}
      </div>

      {/* No due cards for selected language */}
      {!visibleCard ? (
        <div className="flex-1 flex flex-col items-center justify-center text-gray-400 gap-3">
          {sessionQueue.length > 0 ? (
            <>
              <div className="text-4xl">⏳</div>
              <p className="text-sm text-center">Cards in learning — step timer hasn't elapsed yet.</p>
              <p className="text-xs text-gray-300">{sessionQueue.length} card{sessionQueue.length !== 1 ? 's' : ''} waiting for step interval</p>
              <p className="text-xs text-gray-300">Auto-refreshing every 5 seconds</p>
            </>
          ) : (
            <>
              <div className="text-4xl">🎉</div>
              <p className="text-sm text-center">All done for {LANGUAGES[selectedLang || '']?.label || selectedLang}!</p>
              <p className="text-xs text-gray-300">{deckItems.length} cards in deck, {dueInDeck} due now</p>
              {newCardsToday >= maxNewCards && (
                <p className="text-xs text-amber-500">New card limit reached ({maxNewCards}/day) — remaining new cards will appear tomorrow</p>
              )}
            </>
          )}
          <button
            onClick={refreshFromDB}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-xl bg-gray-100 text-gray-600 text-sm font-medium hover:bg-gray-200 transition-colors"
          >
            <ReloadOutlined /> Refresh
          </button>
        </div>
      ) : (
        <>
          {/* Flashcard */}
          <div className="flex-1 flex flex-col items-center justify-center pb-6">
            <div
              onClick={() => setRevealed(true)}
              className="w-full max-w-sm bg-white rounded-2xl shadow-md p-8 text-center cursor-pointer hover:shadow-lg active:shadow transition-shadow"
            >
              {/* State badge & difficulty indicator */}
              <div className="flex items-center justify-center gap-2 mb-4">
                <span className={`text-xs px-2 py-0.5 rounded-full text-white ${
                  visibleCard.state === 'new' ? 'bg-purple-500' :
                  visibleCard.state === 'learning' ? 'bg-blue-500' :
                  visibleCard.state === 'relearning' ? 'bg-orange-500' :
                  'bg-green-500'
                }`}>
                  {stateLabel((visibleCard.state || 'new') as CardState)}
                </span>
                <div
                  className="w-16 h-2 rounded-full"
                  style={{ backgroundColor: difficultyColor(visibleCard.EF, visibleCard.R) }}
                />
              </div>

              <div className="text-2xl font-semibold mb-1">{visibleCard?.content || ''}</div>
              {visibleCard.tags && (
                <span className={`inline-block text-xs px-2 py-0.5 rounded-full text-white ${difficultyBgClass(visibleCard.EF, visibleCard.R)} mt-1`}>
                  {visibleCard.tags}
                </span>
              )}

              {!revealed ? (
                <div className="text-sm text-gray-400 mt-4">Tap to reveal</div>
              ) : (
                <>
                  {/* Detail section */}
                  {renderDetailContent(visibleCard.detail)}

                  {/* SRS stats */}
                  <div className="mt-4 pt-3 border-t border-gray-100">
                    <div className="flex items-center justify-center gap-4 text-xs text-gray-400">
                      <span>Reps: {visibleCard.R}</span>
                      <span>Interval: {visibleCard.I}d</span>
                      <span>EF: {visibleCard.EF.toFixed(2)}</span>
                      {visibleCard.lapse_count > 0 && (
                        <span>Lapses: {visibleCard.lapse_count}</span>
                      )}
                    </div>
                    {visibleCard.next_review && (
                      <div className="text-xs text-gray-300 mt-1">
                        Next review: {new Date(visibleCard.next_review * 1000).toLocaleDateString()}
                      </div>
                    )}
                  </div>
                </>
              )}
            </div>
          </div>

          {/* Grade buttons - fixed at bottom for one-handed reach */}
          <div className="fixed bottom-14 left-0 right-0 px-4 pt-3 pb-4 z-40">
            {revealed ? (
              <div className="grid grid-cols-4 gap-2">
                {gradeOptions.map((g) => (
                  <button
                    key={g.grade}
                    onClick={() => handleGrade(g.grade)}
                    className={`flex flex-col items-center justify-center gap-1 py-3 rounded-2xl text-white font-medium ${g.color} hover:opacity-90 active:opacity-80 transition-opacity shadow-sm`}
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
                  Show Answer
                </button>
              </div>
            )}
          </div>
        </>
      )}
    </div>
  )
}
