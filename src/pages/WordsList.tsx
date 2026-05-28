import { useEffect, useState, useRef, useCallback } from 'react'
import { useNavigate } from 'react-router-dom'
import { invoke } from '@tauri-apps/api/core'
import {
  ArrowLeftOutlined,
  EditOutlined,
  DeleteOutlined,
  CloseOutlined,
  FilterOutlined,
  SortAscendingOutlined,
  SortDescendingOutlined,
  SearchOutlined,
} from '@ant-design/icons'
import type { ReviewItem } from '../lib/types'
import { parseDetailJson } from '../lib/types'
import EditWordForm from '../components/EditWordForm'

const PAGE_SIZE = 20

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

export default function WordsList() {
  const navigate = useNavigate()

  const [items, setItems] = useState<ReviewItem[]>([])
  const [total, setTotal] = useState(0)
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
  const [offset, setOffset] = useState(0)

  // Detail view
  const [selectedId, setSelectedId] = useState<string | null>(null)

  // Edit state
  const [editingId, setEditingId] = useState<string | null>(null)

  // Delete confirmation
  const [deletingId, setDeletingId] = useState<string | null>(null)

  // Scroll ref
  const scrollRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    loadItems(false)
  }, [])

  const loadItems = useCallback(async (append: boolean) => {
    setLoading(true)
    const currentOffset = append ? offset + PAGE_SIZE : 0
    try {
      const result = await invoke<{ items: ReviewItem[]; total: number }>('db_search_review_items', {
        query: searchQuery || null,
        language: filterLang,
        state: filterState,
        difficulty: filterDifficulty !== 'all' ? filterDifficulty : null,
        sort: sortOrder,
        offset: currentOffset,
        limit: PAGE_SIZE,
      })
      if (append) {
        setItems((prev) => [...prev, ...result.items])
      } else {
        setItems(result.items)
      }
      setOffset(currentOffset)
      setTotal(result.total)
    } catch (e) {
      console.error('Failed to load items:', e)
    }
    setLoading(false)
  }, [searchQuery, filterLang, filterState, filterDifficulty, sortOrder, offset])

  // Reload on filter/sort change (non-append)
  useEffect(() => {
    loadItems(false)
  }, [searchQuery, filterLang, filterState, filterDifficulty, sortOrder])

  const hasMore = items.length < total

  // Infinite scroll: load more when scrolling near bottom
  const handleScroll = useCallback(() => {
    const el = scrollRef.current
    if (!el || !hasMore || loading) return
    if (el.scrollHeight - el.scrollTop - el.clientHeight < 150) {
      loadItems(true)
    }
  }, [hasMore, loading, loadItems])

  async function handleDelete(id: string) {
    try {
      await invoke('db_delete_review_item', { id })
      setItems((prev) => prev.filter((item) => item.id !== id))
      if (selectedId === id) setSelectedId(null)
      setDeletingId(null)
    } catch (e) {
      console.error('Failed to delete item:', e)
    }
  }

  // Available languages for filter
  const availableLangs = Object.keys(LANGUAGES)

  // Selected item for detail view
  const selectedItem = selectedId ? items.find((i) => i.id === selectedId) : null

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
            onClick={() => { setSelectedId(null); loadItems(false) }}
            className="w-9 h-9 flex items-center justify-center rounded-full hover:bg-gray-100 transition-colors"
          >
            <ArrowLeftOutlined />
          </button>
          <h2 className="text-lg font-semibold flex-1">Word Detail</h2>
          <button
            onClick={() => setEditingId(selectedItem.id)}
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
                {selectedItem.state_label}
              </span>
              <div
                className="w-16 h-2 rounded-full"
                style={{ backgroundColor: selectedItem.difficulty_color }}
              />
            </div>

            <div className="text-2xl font-semibold mb-1">{selectedItem.content}</div>
            {selectedItem.difficulty_label && (
              <span
                className="inline-block text-xs px-2 py-0.5 rounded-full text-white mt-1"
                style={{ backgroundColor: selectedItem.difficulty_color }}
              >
                {selectedItem.difficulty_label}
              </span>
            )}
            {selectedItem.language && LANGUAGES[selectedItem.language] && (
              <div className="mt-2 text-sm text-gray-500">
                {LANGUAGES[selectedItem.language].flag} {LANGUAGES[selectedItem.language].label}
              </div>
            )}

            {/* Detail section */}
            {renderDetailContent(selectedItem.detail)}

            {/* Mnemonic section */}
            <div className="mt-3 pt-2 border-t border-gray-100 text-left">
              <span className="text-xs text-gray-400">Mnemonic</span>
              {selectedItem.mnemonic ? (
                <p className="text-sm text-amber-700 bg-amber-50 p-2 rounded-lg mt-1">{selectedItem.mnemonic}</p>
              ) : (
                <p className="text-xs text-gray-300 mt-1">No mnemonic. Edit this word to generate one.</p>
              )}
            </div>

            {/* SRS stats */}
            <div className="mt-4 pt-3 border-t border-gray-100">
              <div className="flex items-center justify-center gap-4 text-xs text-gray-400">
                <span>Reps: {selectedItem.R}</span>
                <span>Stability: {selectedItem.fsrs_stability != null ? `${selectedItem.fsrs_stability.toFixed(1)}d` : `${selectedItem.I}d`}</span>
                <span>Difficulty: {selectedItem.fsrs_difficulty != null ? selectedItem.fsrs_difficulty.toFixed(1) : '—'}</span>
                {selectedItem.lapse_count > 0 && <span>Lapses: {selectedItem.lapse_count}</span>}
              </div>
              {selectedItem.next_review && (
                <div className="text-xs text-gray-300 mt-1">
                  Next review: {selectedItem.next_review_formatted}
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
    const item = items.find((i) => i.id === editingId)
    if (!item) {
      setEditingId(null)
      return null
    }
    return (
      <EditWordForm
        item={item}
        title="Edit Word"
        onCancel={() => setEditingId(null)}
        onSave={async (data) => {
          if (data.content) {
            await invoke('db_update_review_item', {
              item: { id: editingId, content: data.content },
            })
          }
          await invoke('db_update_review_detail', { id: editingId, detail: data.detail })
          await invoke('db_update_mnemonic', { id: editingId, mnemonic: data.mnemonic })
          await loadItems(false)
          setSelectedId(editingId)
          setEditingId(null)
        }}
      />
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
          <span className="text-xs text-gray-400">{total} words</span>
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
      ) : items.length === 0 ? (
        <div className="flex-1 flex flex-col items-center justify-center text-gray-400 gap-3">
          <div className="text-4xl">📚</div>
          <p className="text-sm">No words found</p>
          {total === 0 ? (
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
          {hasMore && offset > 0 && (
            <div className="flex items-center justify-center py-2 text-gray-400">
              <span className="text-xs">{total - items.length} more words available</span>
            </div>
          )}

          <div className="bg-white rounded-xl shadow-sm overflow-hidden">
            {items.map((item, idx) => {
              return (
                <div
                  key={item.id}
                  onClick={() => setSelectedId(item.id)}
                  className={`flex items-center gap-3 px-4 py-3 cursor-pointer hover:bg-gray-50 active:bg-gray-100 transition-colors ${
                    idx < items.length - 1 ? 'border-b border-gray-50' : ''
                  }`}
                >
                  {/* Difficulty indicator */}
                  <div
                    className="w-1 h-8 rounded-full shrink-0"
                    style={{ backgroundColor: item.difficulty_color }}
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
                        {item.state_label}
                      </span>
                      {item.language && LANGUAGES[item.language] && (
                        <span className="text-xs shrink-0">{LANGUAGES[item.language].flag}</span>
                      )}
                    </div>
                    <div className="flex items-center gap-2 mt-0.5">
                      {item.difficulty_label && (
                        <span
                          className="text-[10px] px-1.5 py-0.5 rounded-full text-white"
                          style={{ backgroundColor: item.difficulty_color }}
                        >
                          {item.difficulty_label}
                        </span>
                      )}
                      <span className="text-[10px] text-gray-300">
                        S {item.fsrs_stability != null ? `${Math.round(item.fsrs_stability)}d` : `${item.I}d`} &middot; D {item.fsrs_difficulty != null ? item.fsrs_difficulty.toFixed(1) : '—'} &middot; R {item.R}
                        {item.lapse_count > 0 && ` \u00B7 ${item.lapse_count} lapse${item.lapse_count > 1 ? 's' : ''}`}
                      </span>
                      {item.next_review && (
                        <span className="text-[10px] text-gray-400 shrink-0">
                          Next Review: {item.next_review_formatted}
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
                          onClick={() => setEditingId(item.id)}
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
                Showing {items.length} of {total} &middot; Scroll down for more
              </span>
            </div>
          )}

          <div className="h-4" />
        </div>
      )}
    </div>
  )
}
