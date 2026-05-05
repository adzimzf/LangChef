import { useEffect, useState, useRef, useCallback, useMemo } from 'react'
import { useNavigate } from 'react-router-dom'
import { invoke } from '@tauri-apps/api/core'
import {
  ArrowLeftOutlined,
  EditOutlined,
  DeleteOutlined,
  CheckOutlined,
  CloseOutlined,
  FilterOutlined,
  SortAscendingOutlined,
  SortDescendingOutlined,
  RobotOutlined,
  SearchOutlined,
} from '@ant-design/icons'
import { difficultyColor, difficultyBgClass, stateLabel } from '../lib/srs'
import type { CardState } from '../lib/srs'

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

const PAGE_SIZE = 20

function formatNextReview(nextReviewSeconds: number): string {
  const now = Date.now()
  const target = nextReviewSeconds * 1000
  const diffMs = target - now
  const isPast = diffMs < 0
  const absDiff = Math.abs(diffMs)

  const MINUTE = 60 * 1000
  const HOUR = 60 * MINUTE
  const DAY = 24 * HOUR
  const WEEK = 7 * DAY
  const MONTH = 30 * DAY
  const YEAR = 365 * DAY

  let value: number
  let unit: string

  if (absDiff < MINUTE) {
    value = Math.floor(absDiff / 1000)
    unit = 's'
  } else if (absDiff < HOUR) {
    value = Math.floor(absDiff / MINUTE)
    unit = 'm'
  } else if (absDiff < DAY) {
    value = Math.floor(absDiff / HOUR)
    unit = 'h'
  } else if (absDiff < WEEK) {
    value = Math.floor(absDiff / DAY)
    unit = 'd'
  } else if (absDiff < MONTH) {
    value = Math.floor(absDiff / WEEK)
    unit = 'w'
  } else if (absDiff < YEAR) {
    value = Math.floor(absDiff / MONTH)
    unit = 'mo'
  } else {
    value = Math.floor(absDiff / YEAR)
    unit = 'y+'
  }

  return isPast ? 'overdue' : `in ${value}${unit}`
}

const LANGUAGES: Record<string, { label: string; flag: string }> = {
  ja: { label: 'Japanese', flag: '\u{1F1EF}\u{1F1F5}' },
  id: { label: 'Indonesian', flag: '\u{1F1EE}\u{1F1E9}' },
}

const STATES: { value: string; label: string; color: string }[] = [
  { value: 'new', label: 'New', color: 'bg-purple-500' },
  { value: 'learning', label: 'Learning', color: 'bg-blue-500' },
  { value: 'review', label: 'Review', color: 'bg-green-500' },
  { value: 'relearning', label: 'Relearning', color: 'bg-orange-500' },
]

type SortOrder = 'hardest' | 'easiest' | 'next_review'

/** Compute an ease score (0 = hardest, 1 = easiest) for sorting */
function easeScore(EF: number, R: number): number {
  const efNorm = Math.min(1, Math.max(0, (EF - 1.3) / (3.0 - 1.3)))
  const rNorm = Math.min(1, Math.max(0, R / 5))
  return 0.6 * efNorm + 0.4 * rNorm
}

function parseDetailJson(detail: string): {
  reading?: string; meaning?: string; part_of_speech?: string; example?: string; example_meaning?: string
} | null {
  try {
    const match = detail.match(/\{[\s\S]*\}/)
    if (!match) return null
    return JSON.parse(match[0])
  } catch {
    return null
  }
}

export default function WordsList() {
  const navigate = useNavigate()

  const [allItems, setAllItems] = useState<ReviewItem[]>([])
  const [loading, setLoading] = useState(true)

  // Search
  const [searchQuery, setSearchQuery] = useState('')

  // Filters
  const [filterLang, setFilterLang] = useState<string | null>(null)
  const [filterState, setFilterState] = useState<string | null>(null)
  const [filterDifficulty, setFilterDifficulty] = useState<'all' | 'hard' | 'medium' | 'easy'>('all')
  const [showFilters, setShowFilters] = useState(false)

  // Sort
  const [sortOrder, setSortOrder] = useState<SortOrder>('hardest')

  // Pagination
  const [page, setPage] = useState(1)

  // Detail view
  const [selectedId, setSelectedId] = useState<string | null>(null)

  // Edit state
  const [editingId, setEditingId] = useState<string | null>(null)
  const [editContent, setEditContent] = useState('')
  const [editDetail, setEditDetail] = useState('')
  const [generatingDetail, setGeneratingDetail] = useState(false)
  // Structured detail form fields
  const [editReading, setEditReading] = useState('')
  const [editMeaning, setEditMeaning] = useState('')
  const [editPartOfSpeech, setEditPartOfSpeech] = useState('')
  const [editExample, setEditExample] = useState('')
  const [editExampleMeaning, setEditExampleMeaning] = useState('')
  const [saveError, setSaveError] = useState<string | null>(null)

  // Delete confirmation
  const [deletingId, setDeletingId] = useState<string | null>(null)

  // Scroll ref
  const scrollRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    loadItems()
  }, [])

  async function loadItems() {
    setLoading(true)
    try {
      const items = await invoke<ReviewItem[]>('db_get_review_items')
      setAllItems(items)
    } catch (e) {
      console.error('Failed to load items:', e)
    }
    setLoading(false)
  }

  // Filter + sort
  const filteredItems = useMemo(() => {
    let items = [...allItems]

    // Search by content (word key)
    if (searchQuery.trim()) {
      const q = searchQuery.trim().toLowerCase()
      items = items.filter((i) => i.content.toLowerCase().includes(q))
    }

    // Filter by language
    if (filterLang) {
      items = items.filter((i) => i.language === filterLang)
    }

    // Filter by state
    if (filterState) {
      items = items.filter((i) => i.state === filterState)
    }

    // Filter by difficulty
    if (filterDifficulty !== 'all') {
      items = items.filter((i) => {
        const ease = easeScore(i.EF, i.R)
        if (filterDifficulty === 'hard') return ease < 0.3
        if (filterDifficulty === 'medium') return ease >= 0.3 && ease < 0.7
        if (filterDifficulty === 'easy') return ease >= 0.7
        return true
      })
    }

    // Sort
    if (sortOrder === 'next_review') {
      items.sort((a, b) => {
        const aVal = a.next_review ?? Infinity
        const bVal = b.next_review ?? Infinity
        return aVal - bVal
      })
    } else if (sortOrder === 'hardest') {
      items.sort((a, b) => easeScore(a.EF, a.R) - easeScore(b.EF, b.R))
    } else {
      items.sort((a, b) => easeScore(b.EF, b.R) - easeScore(a.EF, a.R))
    }

    return items
  }, [allItems, searchQuery, filterLang, filterState, filterDifficulty, sortOrder])

  // Visible items for current page
  const visibleItems = useMemo(() => {
    return filteredItems.slice(0, page * PAGE_SIZE)
  }, [filteredItems, page])

  const hasMore = visibleItems.length < filteredItems.length

  // Reset page when filters/sort change
  useEffect(() => {
    setPage(1)
  }, [searchQuery, filterLang, filterState, filterDifficulty, sortOrder])

  // Infinite scroll: load more when scrolling near the bottom
  const handleScroll = useCallback(() => {
    const el = scrollRef.current
    if (!el || !hasMore) return
    // When user scrolls near the bottom, load more
    if (el.scrollHeight - el.scrollTop - el.clientHeight < 150) {
      setPage((p) => p + 1)
    }
  }, [hasMore])

  async function handleDelete(id: string) {
    try {
      await invoke('db_delete_review_item', { id })
      setAllItems((prev) => prev.filter((item) => item.id !== id))
      if (selectedId === id) setSelectedId(null)
      setDeletingId(null)
    } catch (e) {
      console.error('Failed to delete item:', e)
    }
  }

  function startEdit(item: ReviewItem) {
    setEditingId(item.id)
    setEditContent(item.content)
    setEditDetail(item.detail || '')
    // Parse detail into form fields
    const parsed = item.detail ? parseDetailJson(item.detail) : null
    setEditReading(parsed?.reading || '')
    setEditMeaning(parsed?.meaning || '')
    setEditPartOfSpeech(parsed?.part_of_speech || '')
    setEditExample(parsed?.example || '')
    setEditExampleMeaning(parsed?.example_meaning || '')
  }

  /** Serialize form fields back to a detail JSON string */
  function serializeDetail(): string {
    const obj: Record<string, string> = {}
    if (editReading) obj.reading = editReading
    if (editMeaning) obj.meaning = editMeaning
    if (editPartOfSpeech) obj.part_of_speech = editPartOfSpeech
    if (editExample) obj.example = editExample
    if (editExampleMeaning) obj.example_meaning = editExampleMeaning
    return Object.keys(obj).length > 0 ? JSON.stringify(obj) : ''
  }

  async function generateDetail() {
    if (!editingId) return
    const item = allItems.find((i) => i.id === editingId)
    if (!item) return
    const apiHost = localStorage.getItem('api_host') || ''
    const apiKey = localStorage.getItem('api_key') || ''
    const model = localStorage.getItem('api_model') || 'gpt-3.5-turbo'
    if (!apiHost || !apiKey) {
      alert('Please configure your API settings first (Settings page).')
      return
    }
    setGeneratingDetail(true)
    try {
      const lang = item.language || 'ja'
      const result = await invoke<string>('db_generate_detail', {
        apiHost, apiKey, model,
        content: item.content,
        language: lang,
      })
      setEditDetail(result)
      // Parse into form fields
      const parsed = parseDetailJson(result)
      setEditReading(parsed?.reading || '')
      setEditMeaning(parsed?.meaning || '')
      setEditPartOfSpeech(parsed?.part_of_speech || '')
      setEditExample(parsed?.example || '')
      setEditExampleMeaning(parsed?.example_meaning || '')
      // Update local state
      setAllItems((prev) =>
        prev.map((i) => (i.id === editingId ? { ...i, detail: result } : i))
      )
    } catch (e) {
      alert(`Failed to generate: ${e}`)
    }
    setGeneratingDetail(false)
  }

  async function saveEdit() {
    if (!editingId) return
    setSaveError(null)
    try {
      const detailValue = serializeDetail()
      // Update content - only if changed and non-empty
      if (editContent.trim()) {
        await invoke('db_update_review_item', {
          item: { id: editingId, content: editContent.trim() },
        })
      }
      // Update detail
      await invoke('db_update_review_detail', { id: editingId, detail: detailValue })
      // Refetch from DB to ensure consistency
      await loadItems()
      // Go to detail view
      setSelectedId(editingId)
      setEditingId(null)
    } catch (e: any) {
      const msg = typeof e === 'string' ? e : e?.message || String(e)
      setSaveError(msg)
    }
  }

  function cancelEdit() {
    setEditingId(null)
    setEditContent('')
    setEditDetail('')
    setEditReading('')
    setEditMeaning('')
    setEditPartOfSpeech('')
    setEditExample('')
    setEditExampleMeaning('')
    setSaveError(null)
  }

  // Get available languages from data
  const availableLangs = useMemo(() => {
    const langs = new Set(allItems.map((i) => i.language).filter(Boolean) as string[])
    return Array.from(langs)
  }, [allItems])

  // Selected item for detail view
  const selectedItem = selectedId ? allItems.find((i) => i.id === selectedId) : null

  function renderDetailContent(detail: string | null) {
    if (!detail) {
      return (
        <div className="mt-3 p-3 bg-gray-50 rounded-xl text-center">
          <p className="text-sm text-gray-400">No detail available.</p>
        </div>
      )
    }

    const parsed = parseDetailJson(detail)
    if (parsed) {
      return (
        <div className="mt-3 space-y-2 text-left">
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
        </div>
      )
    }

    return (
      <div className="mt-3 text-left">
        <p className="text-sm text-gray-600 whitespace-pre-wrap">{detail}</p>
      </div>
    )
  }

  // ─── Detail view (full screen overlay) ─────────────────────────────────
  if (selectedItem && editingId !== selectedItem.id) {
    return (
      <div className="flex flex-col h-full -m-4">
        <div className="shrink-0 sticky top-0 z-10 bg-gray-50 flex items-center gap-3 px-4 pt-4 pb-2">
          <button
            onClick={() => { setSelectedId(null); loadItems() }}
            className="w-9 h-9 flex items-center justify-center rounded-full hover:bg-gray-100 transition-colors"
          >
            <ArrowLeftOutlined />
          </button>
          <h2 className="text-lg font-semibold flex-1">Word Detail</h2>
          <button
            onClick={() => startEdit(selectedItem)}
            className="flex items-center gap-1 px-3 py-1.5 rounded-lg bg-blue-50 text-blue-600 text-sm font-medium hover:bg-blue-100 transition-colors"
          >
            <EditOutlined /> Edit
          </button>
          {deletingId === selectedItem.id ? (
            <div className="flex items-center gap-1">
              <button
                onClick={() => handleDelete(selectedItem.id)}
                className="px-3 py-1.5 rounded-lg bg-red-500 text-white text-sm font-medium hover:bg-red-600 transition-colors"
              >
                Confirm
              </button>
              <button
                onClick={() => setDeletingId(null)}
                className="px-3 py-1.5 rounded-lg bg-gray-100 text-gray-600 text-sm font-medium hover:bg-gray-200 transition-colors"
              >
                Cancel
              </button>
            </div>
          ) : (
            <button
              onClick={() => setDeletingId(selectedItem.id)}
              className="flex items-center gap-1 px-3 py-1.5 rounded-lg bg-red-50 text-red-600 text-sm font-medium hover:bg-red-100 transition-colors"
            >
              <DeleteOutlined /> Delete
            </button>
          )}
        </div>

        {/* Card */}
        <div className="flex-1 flex flex-col items-center justify-start pt-4 px-4 overflow-y-auto">
          <div className="w-full max-w-sm bg-white rounded-2xl shadow-md p-8 text-center">
            {/* State badge & difficulty indicator */}
            <div className="flex items-center justify-center gap-2 mb-4">
              <span
                className={`text-xs px-2 py-0.5 rounded-full text-white ${
                  selectedItem.state === 'new'
                    ? 'bg-purple-500'
                    : selectedItem.state === 'learning'
                    ? 'bg-blue-500'
                    : selectedItem.state === 'relearning'
                    ? 'bg-orange-500'
                    : 'bg-green-500'
                }`}
              >
                {stateLabel((selectedItem.state || 'new') as CardState)}
              </span>
              <div
                className="w-16 h-2 rounded-full"
                style={{ backgroundColor: difficultyColor(selectedItem.EF, selectedItem.R) }}
              />
            </div>

            <div className="text-2xl font-semibold mb-1">{selectedItem.content}</div>
            {selectedItem.tags && (
              <span
                className={`inline-block text-xs px-2 py-0.5 rounded-full text-white ${difficultyBgClass(
                  selectedItem.EF,
                  selectedItem.R
                )} mt-1`}
              >
                {selectedItem.tags}
              </span>
            )}
            {selectedItem.language && LANGUAGES[selectedItem.language] && (
              <div className="mt-2 text-sm text-gray-500">
                {LANGUAGES[selectedItem.language].flag} {LANGUAGES[selectedItem.language].label}
              </div>
            )}

            {/* Detail section */}
            {renderDetailContent(selectedItem.detail)}

            {/* SRS stats */}
            <div className="mt-4 pt-3 border-t border-gray-100">
              <div className="flex items-center justify-center gap-4 text-xs text-gray-400">
                <span>Reps: {selectedItem.R}</span>
                <span>Interval: {selectedItem.I}d</span>
                <span>EF: {selectedItem.EF.toFixed(2)}</span>
                {selectedItem.lapse_count > 0 && <span>Lapses: {selectedItem.lapse_count}</span>}
              </div>
              {selectedItem.next_review && (
                <div className="text-xs text-gray-300 mt-1">
                  Next review: {formatNextReview(selectedItem.next_review)}
                </div>
              )}
            </div>
          </div>
        </div>
      </div>
    )
  }

  // ─── Edit view (full screen overlay) ─────────────────────────────────
  if (editingId) {
    const item = allItems.find((i) => i.id === editingId)
    const hasDetail = editReading || editMeaning || editPartOfSpeech || editExample || editExampleMeaning || editDetail

    return (
      <div className="flex flex-col h-full -m-4">
        <div className="shrink-0 sticky top-0 z-10 bg-gray-50 flex items-center gap-3 px-4 pt-4 pb-2">
          <button
            onClick={cancelEdit}
            className="w-9 h-9 flex items-center justify-center rounded-full hover:bg-gray-100 transition-colors"
          >
            <ArrowLeftOutlined />
          </button>
          <h2 className="text-lg font-semibold">Edit Word</h2>
        </div>

        <div className="space-y-4 flex-1 overflow-y-auto px-4">
          <div>
            <label className="text-sm text-gray-500 mb-1 block">Content</label>
            <input
              type="text"
              value={editContent}
              onChange={(e) => setEditContent(e.target.value)}
              className="w-full px-3 py-2 text-sm border border-gray-200 rounded-xl outline-none focus:border-blue-400"
              placeholder="Word or phrase"
            />
          </div>

          <div>
            <div className="flex items-center justify-between mb-1">
              <label className="text-sm text-gray-500">Detail</label>
              {!hasDetail && (
                <button
                  onClick={generateDetail}
                  disabled={generatingDetail}
                  className="flex items-center gap-1 px-2.5 py-1 rounded-lg bg-blue-500 text-white text-xs font-medium hover:bg-blue-600 transition-colors disabled:opacity-50"
                >
                  <RobotOutlined />
                  {generatingDetail ? 'Generating...' : 'Generate with AI'}
                </button>
              )}
            </div>

            {hasDetail ? (
              /* Structured form fields for editing detail */
              <div className="space-y-3 bg-gray-50 rounded-xl p-3">
                <div>
                  <label className="text-xs text-gray-400 block mb-0.5">Reading</label>
                  <input
                    type="text"
                    value={editReading}
                    onChange={(e) => setEditReading(e.target.value)}
                    className="w-full px-2.5 py-1.5 text-sm border border-gray-200 rounded-lg outline-none focus:border-blue-400 bg-white"
                    placeholder="Pronunciation in IPA or romaji"
                  />
                </div>
                <div>
                  <label className="text-xs text-gray-400 block mb-0.5">Meaning</label>
                  <input
                    type="text"
                    value={editMeaning}
                    onChange={(e) => setEditMeaning(e.target.value)}
                    className="w-full px-2.5 py-1.5 text-sm border border-gray-200 rounded-lg outline-none focus:border-blue-400 bg-white"
                    placeholder="English translation"
                  />
                </div>
                <div>
                  <label className="text-xs text-gray-400 block mb-0.5">Part of speech</label>
                  <input
                    type="text"
                    value={editPartOfSpeech}
                    onChange={(e) => setEditPartOfSpeech(e.target.value)}
                    className="w-full px-2.5 py-1.5 text-sm border border-gray-200 rounded-lg outline-none focus:border-blue-400 bg-white"
                    placeholder="noun / verb / adjective / etc."
                  />
                </div>
                <div>
                  <label className="text-xs text-gray-400 block mb-0.5">Example sentence</label>
                  <textarea
                    value={editExample}
                    onChange={(e) => setEditExample(e.target.value)}
                    className="w-full px-2.5 py-1.5 text-sm border border-gray-200 rounded-lg outline-none focus:border-blue-400 bg-white resize-none"
                    placeholder="A natural example sentence"
                    rows={2}
                  />
                </div>
                <div>
                  <label className="text-xs text-gray-400 block mb-0.5">Example meaning</label>
                  <textarea
                    value={editExampleMeaning}
                    onChange={(e) => setEditExampleMeaning(e.target.value)}
                    className="w-full px-2.5 py-1.5 text-sm border border-gray-200 rounded-lg outline-none focus:border-blue-400 bg-white resize-none"
                    placeholder="English translation of the example"
                    rows={2}
                  />
                </div>
                <button
                  onClick={generateDetail}
                  disabled={generatingDetail}
                  className="flex items-center gap-1 px-2.5 py-1.5 rounded-lg bg-gray-200 text-gray-600 text-xs font-medium hover:bg-gray-300 transition-colors disabled:opacity-50"
                >
                  <RobotOutlined />
                  {generatingDetail ? 'Regenerating...' : 'Regenerate with AI'}
                </button>
              </div>
            ) : (
              /* Empty state */
              <div className="bg-gray-50 rounded-xl p-6 text-center">
                <p className="text-sm text-gray-400 mb-1">No detail yet</p>
                <p className="text-xs text-gray-300">Click "Generate with AI" above or fill in the content and save first.</p>
              </div>
            )}
          </div>
        </div>

        <div className="shrink-0 px-4 pt-2 pb-4 bg-gray-50">
          {saveError && (
            <div className="mb-2 p-2.5 rounded-xl bg-red-50 border border-red-100 text-red-700 text-sm">
              {saveError}
            </div>
          )}
          <button
            onClick={saveEdit}
            className="w-full py-3 rounded-xl bg-blue-500 text-white font-medium hover:bg-blue-600 active:bg-blue-700 transition-colors flex items-center justify-center gap-2"
          >
            <CheckOutlined /> Save
          </button>
        </div>
      </div>
    )
  }

  // ─── Main list view ─────────────────────────────────────────
  return (
    <div className="flex flex-col h-full -mx-4">
      {/* Header - sticky */}
      <div className="shrink-0 sticky top-0 z-10 bg-gray-50 px-4 pt-4 pb-2">
        <div className="flex items-center gap-3 mb-3">
          <button
            onClick={() => navigate('/')}
            className="w-9 h-9 flex items-center justify-center rounded-full hover:bg-gray-100 transition-colors"
          >
            <ArrowLeftOutlined />
          </button>
          <h2 className="text-lg font-semibold flex-1">Words List</h2>
          <span className="text-xs text-gray-400">{filteredItems.length} words</span>
        </div>

        {/* Search */}
        <div className="relative mb-2">
          <SearchOutlined className="absolute left-3 top-1/2 -translate-y-1/2 text-gray-400" />
          <input
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder="Search by word..."
            className="w-full pl-9 pr-3 py-1.5 rounded-xl text-sm bg-gray-100 border-none outline-none focus:ring-2 focus:ring-blue-300 placeholder-gray-400"
          />
          {searchQuery && (
            <button
              onClick={() => setSearchQuery('')}
              className="absolute right-2 top-1/2 -translate-y-1/2 text-gray-400 hover:text-gray-600"
            >
              <CloseOutlined style={{ fontSize: 12 }} />
            </button>
          )}
        </div>

        {/* Sort & Filter toggle */}
        <div className="flex items-center gap-2 mb-2">
          <button
            onClick={() => setSortOrder(sortOrder === 'hardest' ? 'easiest' : 'hardest')}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-xl text-sm font-medium bg-gray-100 text-gray-600 hover:bg-gray-200 transition-colors"
          >
            {sortOrder === 'hardest' ? (
              <SortDescendingOutlined />
            ) : (
              <SortAscendingOutlined />
            )}
            {sortOrder === 'hardest' ? 'Hardest first' : 'Easiest first'}
          </button>
          <button
            onClick={() => setSortOrder(sortOrder === 'next_review' ? 'hardest' : 'next_review')}
            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-xl text-sm font-medium transition-colors ${
              sortOrder === 'next_review'
                ? 'bg-blue-500 text-white'
                : 'bg-gray-100 text-gray-600 hover:bg-gray-200'
            }`}
          >
            <SortAscendingOutlined />
            Next review
          </button>
          <button
            onClick={() => setShowFilters(!showFilters)}
            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-xl text-sm font-medium transition-colors ${
              showFilters || filterLang || filterState || filterDifficulty !== 'all'
                ? 'bg-blue-500 text-white'
                : 'bg-gray-100 text-gray-600 hover:bg-gray-200'
            }`}
          >
            <FilterOutlined /> Filter
            {(filterLang || filterState || filterDifficulty !== 'all') && (
              <span className="w-4 h-4 rounded-full bg-white text-blue-500 text-[10px] flex items-center justify-center font-bold">
                {[filterLang, filterState, filterDifficulty !== 'all' ? 'x' : null].filter(Boolean).length}
              </span>
            )}
          </button>
        </div>

        {/* Filters panel */}
        {showFilters && (
          <div className="bg-white rounded-xl p-3 shadow-sm mb-2 space-y-3">
            {/* Language filter */}
            <div>
              <label className="text-xs text-gray-400 block mb-1">Language</label>
              <div className="flex gap-1.5 flex-wrap">
                <button
                  onClick={() => setFilterLang(null)}
                  className={`px-2.5 py-1 rounded-lg text-xs font-medium transition-colors ${
                    !filterLang ? 'bg-blue-500 text-white' : 'bg-gray-100 text-gray-600 hover:bg-gray-200'
                  }`}
                >
                  All
                </button>
                {availableLangs.map((lang) => (
                  <button
                    key={lang}
                    onClick={() => setFilterLang(lang)}
                    className={`px-2.5 py-1 rounded-lg text-xs font-medium transition-colors ${
                      filterLang === lang ? 'bg-blue-500 text-white' : 'bg-gray-100 text-gray-600 hover:bg-gray-200'
                    }`}
                  >
                    {LANGUAGES[lang]?.flag} {LANGUAGES[lang]?.label || lang}
                  </button>
                ))}
              </div>
            </div>

            {/* State filter */}
            <div>
              <label className="text-xs text-gray-400 block mb-1">State</label>
              <div className="flex gap-1.5 flex-wrap">
                <button
                  onClick={() => setFilterState(null)}
                  className={`px-2.5 py-1 rounded-lg text-xs font-medium transition-colors ${
                    !filterState ? 'bg-blue-500 text-white' : 'bg-gray-100 text-gray-600 hover:bg-gray-200'
                  }`}
                >
                  All
                </button>
                {STATES.map((s) => (
                  <button
                    key={s.value}
                    onClick={() => setFilterState(s.value)}
                    className={`px-2.5 py-1 rounded-lg text-xs font-medium transition-colors ${
                      filterState === s.value ? 'bg-blue-500 text-white' : 'bg-gray-100 text-gray-600 hover:bg-gray-200'
                    }`}
                  >
                    {s.label}
                  </button>
                ))}
              </div>
            </div>

            {/* Difficulty filter */}
            <div>
              <label className="text-xs text-gray-400 block mb-1">Difficulty</label>
              <div className="flex gap-1.5">
                {(
                  [
                    { value: 'all', label: 'All' },
                    { value: 'hard', label: 'Hard' },
                    { value: 'medium', label: 'Medium' },
                    { value: 'easy', label: 'Easy' },
                  ] as const
                ).map((d) => (
                  <button
                    key={d.value}
                    onClick={() => setFilterDifficulty(d.value)}
                    className={`px-2.5 py-1 rounded-lg text-xs font-medium transition-colors ${
                      filterDifficulty === d.value ? 'bg-blue-500 text-white' : 'bg-gray-100 text-gray-600 hover:bg-gray-200'
                    }`}
                  >
                    {d.label}
                  </button>
                ))}
              </div>
            </div>
          </div>
        )}
      </div>

      {/* Word list */}
      {loading ? (
        <div className="flex items-center justify-center py-16 text-gray-400">
          <p className="text-sm">Loading words...</p>
        </div>
      ) : filteredItems.length === 0 ? (
        <div className="flex-1 flex flex-col items-center justify-center text-gray-400 gap-3">
          <div className="text-4xl">📚</div>
          <p className="text-sm">No words found</p>
          {allItems.length === 0 ? (
            <p className="text-xs text-gray-300">Start chatting and flag difficult words to build your deck.</p>
          ) : (
            <button
              onClick={() => {
                setFilterLang(null)
                setFilterState(null)
                setFilterDifficulty('all')
              }}
              className="text-xs text-blue-500 hover:underline"
            >
              Clear filters
            </button>
          )}
        </div>
      ) : (
        <div
          ref={scrollRef}
          onScroll={handleScroll}
          className="flex-1 overflow-y-auto px-4"
        >
          {/* Load more indicator (top) */}
          {hasMore && page > 1 && (
            <div className="flex items-center justify-center py-2 text-gray-400">
              <span className="text-xs">{filteredItems.length - visibleItems.length} more words below</span>
            </div>
          )}

          <div className="bg-white rounded-xl shadow-sm overflow-hidden">
            {visibleItems.map((item, idx) => {
              const ease = easeScore(item.EF, item.R)
              return (
                <div
                  key={item.id}
                  onClick={() => setSelectedId(item.id)}
                  className={`flex items-center gap-3 px-4 py-3 cursor-pointer hover:bg-gray-50 active:bg-gray-100 transition-colors ${
                    idx < visibleItems.length - 1 ? 'border-b border-gray-50' : ''
                  }`}
                >
                  {/* Difficulty indicator */}
                  <div
                    className="w-1 h-8 rounded-full shrink-0"
                    style={{ backgroundColor: difficultyColor(item.EF, item.R) }}
                  />

                  {/* Word content */}
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2">
                      <span className="font-medium text-sm truncate">{item.content}</span>
                      <span
                        className={`text-[10px] px-1.5 py-0.5 rounded-full text-white shrink-0 ${
                          item.state === 'new'
                            ? 'bg-purple-500'
                            : item.state === 'learning'
                            ? 'bg-blue-500'
                            : item.state === 'relearning'
                            ? 'bg-orange-500'
                            : 'bg-green-500'
                        }`}
                      >
                        {stateLabel((item.state || 'new') as CardState)}
                      </span>
                      {item.language && LANGUAGES[item.language] && (
                        <span className="text-xs shrink-0">{LANGUAGES[item.language].flag}</span>
                      )}
                    </div>
                    <div className="flex items-center gap-2 mt-0.5">
                      {item.tags && (
                        <span
                          className={`text-[10px] px-1.5 py-0.5 rounded-full text-white ${difficultyBgClass(
                            item.EF,
                            item.R
                          )}`}
                        >
                          {item.tags}
                        </span>
                      )}
                      <span className="text-[10px] text-gray-300">
                        EF {item.EF.toFixed(2)} &middot; R {item.R}
                        {item.lapse_count > 0 && ` \u00B7 ${item.lapse_count} lapse${item.lapse_count > 1 ? 's' : ''}`}
                      </span>
                      {item.next_review && (
                        <span className="text-[10px] text-gray-400 shrink-0">
                          Next Review: {formatNextReview(item.next_review)}
                        </span>
                      )}
                    </div>
                  </div>

                  {/* Action buttons */}
                  <div className="flex items-center gap-1 shrink-0" onClick={(e) => e.stopPropagation()}>
                    {deletingId === item.id ? (
                      <div className="flex items-center gap-1">
                        <button
                          onClick={() => handleDelete(item.id)}
                          className="px-2 py-1 rounded-lg bg-red-500 text-white text-[10px] font-medium hover:bg-red-600 transition-colors"
                        >
                          Confirm
                        </button>
                        <button
                          onClick={() => setDeletingId(null)}
                          className="px-2 py-1 rounded-lg bg-gray-100 text-gray-600 text-[10px] font-medium hover:bg-gray-200 transition-colors"
                        >
                          Cancel
                        </button>
                      </div>
                    ) : (
                      <>
                        <button
                          onClick={() => startEdit(item)}
                          className="w-7 h-7 flex items-center justify-center rounded-lg text-gray-400 hover:text-blue-500 hover:bg-blue-50 transition-colors"
                          title="Edit"
                        >
                          <EditOutlined className="text-xs" />
                        </button>
                        <button
                          onClick={() => setDeletingId(item.id)}
                          className="w-7 h-7 flex items-center justify-center rounded-lg text-gray-400 hover:text-red-500 hover:bg-red-50 transition-colors"
                          title="Delete"
                        >
                          <DeleteOutlined className="text-xs" />
                        </button>
                      </>
                    )}
                  </div>
                </div>
              )
            })}
          </div>

          {/* Load more indicator (bottom) */}
          {hasMore && (
            <div className="flex items-center justify-center py-3 text-gray-400">
              <span className="text-xs">
                Showing {visibleItems.length} of {filteredItems.length} &middot; Scroll down for more
              </span>
            </div>
          )}

          <div className="h-4" />
        </div>
      )}
    </div>
  )
}
