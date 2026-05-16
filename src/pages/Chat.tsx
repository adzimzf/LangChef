import { useEffect, useMemo, useRef, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { splitTappable, isWordSegment } from '../lib/unknown'
import { renderFurigana, hasFuriganaAnnotations, stripFurigana } from '../lib/furigana'
import { difficultyColor } from '../lib/srs'
import { JLPT_LEVELS, DEFAULT_JLPT_LEVEL, JLPT_LEVEL_STORAGE_KEY } from '../lib/jlpt'
import type { JLPTLevel } from '../lib/jlpt'
import UserMessageFeedback, { type UserFeedback } from '../components/UserMessageFeedback'
import { Input } from 'antd'
const { TextArea } = Input
import { ArrowLeftOutlined, SendOutlined, InfoCircleOutlined, CloseOutlined, PlusOutlined, EditOutlined, LeftOutlined, RightOutlined } from '@ant-design/icons'
import { useNavigate, useSearchParams } from 'react-router-dom'
import ReactMarkdown from 'react-markdown'

type Msg = { id: number | null, role: 'user' | 'assistant' | 'system', content: string, parentMessageId?: number | null, isActivePath?: boolean }

type BranchPointInfo = {
  siblings: Msg[]        // All sibling messages at this branch point
  selectedIndex: number  // Index of the currently selected sibling
}

const LANGUAGES: Record<string, { label: string, flag: string }> = {
  ja: { label: 'Japanese', flag: '🇯🇵' },
  id: { label: 'Indonesian', flag: '🇮🇩' },
}

const SUGGESTIONS = [
  'Random Topic',
  'Travel',
  'Food',
  'Work',
  'Daily routine',
  'Hobbies',
  'Entertainment',
  'Technology',
  'Health & Wellness',
  'Learning & Education',
  'Relationships',
  'Current Events',
  'Nature & Environment',
  'Art & Culture',
  'Personal Finance',
  'Home & Living',
  'Sports & Recreation',
  'Future & Aspirations'
];

const MAX_REVIEWS_DEFAULT = 100
const MAX_REVIEWS_KEY = 'max_reviews_per_day'
const MAX_NEW_CARDS_DEFAULT = 20
const MAX_NEW_CARDS_KEY = 'max_new_cards_per_day'
const MAX_DIFFICULT_WORDS_KEY = 'max_difficult_words'
const MAX_DIFFICULT_WORDS_DEFAULT = 10
const MAX_KNOWN_WORDS_KEY = 'max_known_words'
const MAX_KNOWN_WORDS_DEFAULT = 20
const WORD_QUEUE_USED_KEY = 'word_queue_used_difficult'

function getMaxReviews(): number {
  const val = localStorage.getItem(MAX_REVIEWS_KEY)
  return val ? parseInt(val, 10) || MAX_REVIEWS_DEFAULT : MAX_REVIEWS_DEFAULT
}

function getMaxNewCards(): number {
  const val = localStorage.getItem(MAX_NEW_CARDS_KEY)
  return val ? parseInt(val, 10) || MAX_NEW_CARDS_DEFAULT : MAX_NEW_CARDS_DEFAULT
}

function getMaxDifficultWords(): number {
  const val = localStorage.getItem(MAX_DIFFICULT_WORDS_KEY)
  return val ? parseInt(val, 10) || MAX_DIFFICULT_WORDS_DEFAULT : MAX_DIFFICULT_WORDS_DEFAULT
}

function getMaxKnownWords(): number {
  const val = localStorage.getItem(MAX_KNOWN_WORDS_KEY)
  return val ? parseInt(val, 10) || MAX_KNOWN_WORDS_DEFAULT : MAX_KNOWN_WORDS_DEFAULT
}

function getChatCustomPrompt(): string {
  return localStorage.getItem('chat_custom_prompt') || ''
}

function getLLMPendingKey(sid: string): string {
  return `llm_pending_${sid}`
}

function isLLMPending(sid: string): boolean {
  return localStorage.getItem(getLLMPendingKey(sid)) !== null
}

function setLLMPending(sid: string): void {
  localStorage.setItem(getLLMPendingKey(sid), String(Date.now()))
}

function clearLLMPending(sid: string): void {
  localStorage.removeItem(getLLMPendingKey(sid))
}

function todayKey(): string {
  return new Date().toISOString().slice(0, 10)
}

function getNewCardsToday(): number {
  const stored = localStorage.getItem(`new_seen_${todayKey()}`)
  return stored ? parseInt(stored, 10) : 0
}

type GrammarDBItem = { id: string; pattern: string; detail: string; language: string; level: string; is_default: number }

type ReviewItemRaw = { id: string; content: string; EF: number; I: number; R: number; tags: string | null; state: string | null; next_review: number | null }

/** Compute an ease score (0 = hardest, 1 = easiest) — same as WordList */
function easeScore(EF: number, R: number): number {
  const efNorm = Math.min(1, Math.max(0, (EF - 1.3) / (3.0 - 1.3)))
  const rNorm = Math.min(1, Math.max(0, R / 5))
  return 0.6 * efNorm + 0.4 * rNorm
}


function TypingIndicator() {
  return (
    <div className="flex mb-2 justify-start">
      <div className="bg-white text-gray-800 rounded-2xl rounded-bl-md shadow-sm px-4 py-3 flex items-center gap-1">
        <span className="typing-dot w-2 h-2 bg-gray-400 rounded-full inline-block" />
        <span className="typing-dot w-2 h-2 bg-gray-400 rounded-full inline-block" />
        <span className="typing-dot w-2 h-2 bg-gray-400 rounded-full inline-block" />
      </div>
    </div>
  )
}

/**
 * Walk the message tree from root, following branchSelection at each branch point.
 * Returns the ordered list of visible messages and metadata about branch points
 * for rendering navigation UI.
 */
function computeVisiblePath(
  messages: Msg[],
  branchSelection: Map<number | null, number>
): {
  visibleMessages: Msg[]
  branchPoints: Map<number | null, BranchPointInfo>
} {
  // Build children lookup: parentMessageId -> array of child messages
  const childrenByParent = new Map<number | null, Msg[]>()
  for (const m of messages) {
    const key = m.parentMessageId ?? null
    const children = childrenByParent.get(key) ?? []
    children.push(m)
    childrenByParent.set(key, children)
  }

  // Walk the tree from root
  const visibleMessages: Msg[] = []
  const branchPoints = new Map<number | null, BranchPointInfo>()
  let currentParentId: number | null = null

  while (true) {
    const children = childrenByParent.get(currentParentId)
    if (!children || children.length === 0) break

    if (children.length === 1) {
      // No branch point — just follow the single child
      const selected = children[0]
      visibleMessages.push(selected)
      if (selected.id === null) break // unsaved message has no children yet
      currentParentId = selected.id
    } else {
      // Branch point — multiple children share the same parent
      // Determine which child to follow:
      //   1. Check branchSelection for user override
      //   2. Fall back to the child with isActivePath = true
      //   3. Fall back to the first child
      let selectedIndex = children.findIndex(c => c.isActivePath === true)
      if (selectedIndex === -1) selectedIndex = 0

      const overrideId = branchSelection.get(currentParentId)
      if (overrideId !== undefined) {
        const overrideIdx = children.findIndex(c => c.id === overrideId)
        if (overrideIdx !== -1) selectedIndex = overrideIdx
      }

      branchPoints.set(currentParentId, {
        siblings: children,
        selectedIndex,
      })

      const selected = children[selectedIndex]
      visibleMessages.push(selected)
      if (selected.id === null) break
      currentParentId = selected.id
    }
  }

  return { visibleMessages, branchPoints }
}

export default function Chat() {
  const navigate = useNavigate()
  const [params] = useSearchParams()
  const initialSid = params.get('sid') || ''
  const lang = params.get('lang') || ''
  const jlptParam = params.get('jlpt') || ''
  const [sessionId, setSessionId] = useState(initialSid)
  const [jlptLevel, setJlptLevel] = useState<JLPTLevel>(() => {
    if (jlptParam && jlptParam in JLPT_LEVELS) return jlptParam as JLPTLevel
    const saved = localStorage.getItem(JLPT_LEVEL_STORAGE_KEY)
    if (saved && saved in JLPT_LEVELS) return saved as JLPTLevel
    return DEFAULT_JLPT_LEVEL
  })
  const [apiHost, setApiHost] = useState('')
  const [apiKey, setApiKey] = useState('')
  const [model, setModel] = useState('')
  const [input, setInput] = useState('')
  const [messages, setMessages] = useState<Msg[]>([])
  const [difficult, setDifficult] = useState<Map<string, { EF: number; R: number; isNew: boolean }>>(new Map())
  const [known, setKnown] = useState<Set<string>>(new Set())
  const [loading, setLoading] = useState(false)
  const [showFurigana, setShowFurigana] = useState(false)
  const [grammarItems, setGrammarItems] = useState<GrammarDBItem[]>([])
  const [selectedGrammar, setSelectedGrammar] = useState<Set<string>>(new Set())
  const [showGrammarPicker, setShowGrammarPicker] = useState(false)
  const [grammarInfoItem, setGrammarInfoItem] = useState<GrammarDBItem | null>(null)
  const [grammarAnalysis, setGrammarAnalysis] = useState<string | null>(null)
  const [grammarAnalysisLoading, setGrammarAnalysisLoading] = useState<number | null>(null)
  const [feedbackMap, setFeedbackMap] = useState<Map<number, UserFeedback>>(new Map())
  const [wordColorsByMessage, setWordColorsByMessage] = useState<Map<number, { tokens: Array<{surface: string, base_form: string}>, colors: Map<string, { EF: number; R: number; isNew: boolean }> }>>(new Map())
  const logRef = useRef<HTMLDivElement | null>(null)
  const inputRef = useRef<any>(null)
  // Tracks which difficult words have already been used in the current rotation cycle.
  // Persisted to localStorage so new chats continue where the last one left off.
  // Uses word content (string) instead of a numeric index, so it survives DB
  // changes like reordering, additions, or removals of difficult words.
  const usedDifficultWords = useRef<Set<string>>(
    (() => {
      try {
        const saved = localStorage.getItem(WORD_QUEUE_USED_KEY)
        if (saved) return new Set<string>(JSON.parse(saved) as string[])
      } catch { /* ignore corrupt data */ }
      return new Set<string>()
    })()
  )
  // Known words still use a simple numeric rotation index (they're shuffled each cycle anyway)
  const knownQueueIndex = useRef(0)
  // Track whether component is mounted — prevents stale state updates after navigation
  const mounted = useRef(true)
  const [keyboardInset, setKeyboardInset] = useState(0)
  // Branch selection: maps parentMessageId to the selected child message ID.
  // Only stores user overrides; defaults come from isActivePath in the DB.
  const [branchSelection, setBranchSelection] = useState<Map<number | null, number>>(new Map())
  const [editingMessageId, setEditingMessageId] = useState<number | null>(null)

  const langInfo = LANGUAGES[lang]

  // Compute visible path through the message tree
  const { visibleMessages, branchPoints } = useMemo(() =>
    computeVisiblePath(messages, branchSelection), [messages, branchSelection])

  // Selected grammar items (full objects) derived from selectedGrammar IDs
  const selectedGrammarItems = useMemo(() => {
    return grammarItems.filter(g => selectedGrammar.has(g.id))
  }, [grammarItems, selectedGrammar])

  // Load selected grammar from DB when session is available (Japanese only)
  useEffect(() => {
    if (lang !== 'ja' || !sessionId) return
    let cancelled = false
    invoke<string[]>('db_get_selected_grammar', { sessionId })
      .then(ids => { if (!cancelled) setSelectedGrammar(new Set(ids)) })
      .catch(() => {})
    return () => { cancelled = true }
  }, [sessionId])

  // Save selected grammar to DB whenever it changes (Japanese only)
  useEffect(() => {
    if (lang !== 'ja' || !sessionId) return
    invoke('db_save_selected_grammar', {
      sessionId,
      grammarItemIds: Array.from(selectedGrammar),
    }).catch(() => {})
  }, [selectedGrammar, sessionId])

  // Clear grammar selection when JLPT level changes (Japanese only)
  useEffect(() => {
    if (lang !== 'ja') return
    setSelectedGrammar(new Set())
  }, [jlptLevel])

  // Close grammar picker on outside click
  useEffect(() => {
    if (!showGrammarPicker) return
    function handleClick(e: MouseEvent) {
      const target = e.target as HTMLElement
      if (target.closest('[data-grammar-picker]') || target.closest('[data-grammar-btn]')) return
      setShowGrammarPicker(false)
    }
    document.addEventListener('click', handleClick)
    return () => document.removeEventListener('click', handleClick)
  }, [showGrammarPicker])

  // Cleanup: mark component as unmounted to prevent stale state updates
  useEffect(() => {
    mounted.current = true
    return () => { mounted.current = false }
  }, [])

  // Load settings
  useEffect(() => {
    const host = localStorage.getItem('api_host') || ''
    const key = localStorage.getItem('api_key') || ''
    const m = localStorage.getItem('api_model') || 'gpt-3.5-turbo'
    setApiHost(host)
    setApiKey(key)
    setModel(m)
  }, [])

  useEffect(() => {
    const vv: any = (window as any).visualViewport
    function updateInset() {
      if (!vv) { setKeyboardInset(0); return }
      const inset = Math.max(0, window.innerHeight - vv.height - vv.offsetTop)
      setKeyboardInset(inset)
    }
    updateInset()
    if (vv) {
      vv.addEventListener('resize', updateInset)
      vv.addEventListener('scroll', updateInset)
    }
    return () => {
      if (vv) {
        vv.removeEventListener('resize', updateInset)
        vv.removeEventListener('scroll', updateInset)
      }
    }
  }, [])

  // Load grammar items from DB for current language + level
  useEffect(() => {
    if (lang !== 'ja') { setGrammarItems([]); return }
    async function load() {
      try {
        const res = await invoke<GrammarDBItem[]>('db_list_grammar_items', { language: lang, level: jlptLevel })
        setGrammarItems(res)
      } catch {
        setGrammarItems([])
      }
    }
    load()
  }, [lang, jlptLevel])

  // Load session messages
  useEffect(() => {
    async function load() {
      if (!sessionId) return
      try {
        const res = await invoke<{ id: number, role: string, content: string, created_at: number, parent_message_id: number | null, is_active_path: number }[]>('db_list_session_messages', { sessionId })
        const msgs: Msg[] = res.map(m => ({
          id: m.id,
          role: m.role as Msg['role'],
          content: m.content,
          parentMessageId: m.parent_message_id,
          isActivePath: m.is_active_path === 1,
        }))
        setMessages(msgs)
        setBranchSelection(new Map())

        // Restore cached feedback for user messages
        for (const m of res) {
          if (m.role === 'user' && m.id) {
            invoke<UserFeedback | null>('db_get_user_feedback', { messageId: m.id })
              .then(fb => { if (fb) setFeedbackMap(prev => new Map(prev).set(m.id!, fb)) })
              .catch(() => {})
          }
        }
      } catch {}
    }
    load()
  }, [sessionId])

  // Restore typing state if LLM was pending when user navigated away
  useEffect(() => {
    if (!sessionId || !isLLMPending(sessionId)) return
    setLoading(true)
    // Baseline: current message count before the in-flight response arrives
    const baseline = messages.length
    // Poll for new assistant message until the in-flight LLM response is saved to DB
    const interval = setInterval(async () => {
      try {
        const res = await invoke<{ id: number, role: string, content: string, created_at: number }[]>('db_list_session_messages', { sessionId })
        // New message(s) appeared — the in-flight LLM response was saved to DB
        if (res.length > baseline) {
          clearInterval(interval)
          clearLLMPending(sessionId)
          setLoading(false)
          setMessages(res.map(m => ({
            id: m.id, role: m.role as Msg['role'], content: m.content,
            parentMessageId: (m as any).parent_message_id ?? null,
            isActivePath: (m as any).is_active_path !== 0,
          })))
        }
      } catch {}
    }, 1500)
    return () => clearInterval(interval)
  }, [sessionId])

  // Load difficult words from review_items for LLM system prompt
  useEffect(() => {
    async function load() {
      try {
        // Query: For LLM system prompt — all items, sorted hardest-first (same as Word List)
        const allItems = await invoke<ReviewItemRaw[]>('db_get_review_items')

        // Sort by ease score (hardest first) — same as WordList default sortOrder
        allItems.sort((a, b) => easeScore(a.EF, a.R) - easeScore(b.EF, b.R))

        const hardMap = new Map<string, { EF: number; R: number; isNew: boolean }>()

        // Classify using easeScore — hard (< 0.3) or medium (0.3–0.7) are difficult
        allItems.forEach(i => {
          const ease = easeScore(i.EF, i.R)
          if (ease < 0.7) {
            const isNew = i.state === 'new' || !i.state
            hardMap.set(i.content.toLowerCase(), { EF: i.EF, R: i.R, isNew })
          }
        })
        setDifficult(hardMap)

        // Known words: easy items (ease >= 0.7) — same metric as WordList
        const knownWords = new Set<string>()
        allItems.filter(i => easeScore(i.EF, i.R) >= 0.7).forEach(i => {
          knownWords.add(i.content.toLowerCase())
        })
        setKnown(knownWords)
      } catch {}
    }
    load()
  }, [])

  // Load known words from DB
  useEffect(() => {
    async function load() {
      try {
        const words = await invoke<string[]>('db_list_known_words')
        setKnown(prev => {
          const next = new Set(prev)
          words.forEach(w => next.add(w.toLowerCase()))
          return next
        })
      } catch {}
    }
    load()
  }, [])

  // Fetch word colors for assistant messages using Lindera tokenization
  useEffect(() => {
    if (lang !== 'ja') return

    async function fetchColors() {
      const assistantMessages = messages.filter(m => m.role === 'assistant' && m.id && !wordColorsByMessage.has(m.id))
      if (assistantMessages.length === 0) return

      for (const msg of assistantMessages) {
        if (!msg.id) continue

        // Strip furigana for tokenization
        const plainText = stripFurigana(msg.content)
        const { tokens, colors } = await getWordColors(plainText)

        setWordColorsByMessage(prev => {
          const next = new Map(prev)
          next.set(msg.id!, { tokens, colors })
          return next
        })
      }
    }

    fetchColors()
  }, [messages, lang])

  // Auto-scroll
  useEffect(() => {
    if (logRef.current) {
      logRef.current.scrollTop = logRef.current.scrollHeight
    }
  }, [messages.length, loading])

  // When keyboard opens, keep latest messages visible
  useEffect(() => {
    if (keyboardInset > 0 && logRef.current) {
      logRef.current.scrollTop = logRef.current.scrollHeight
    }
  }, [keyboardInset])

  async function markWordDifficult(word: string) {
    const w = word.toLowerCase()
    const next = new Map(difficult)
    next.set(w, { EF: 2.5, R: 0, isNew: true })
    setDifficult(next)
    try {
      await invoke('db_upsert_review_item', { content: w, language: lang || null })
    } catch (e) {
      console.error('Failed to save difficult word to DB:', e)
      // Roll back local state if DB write fails
      const reverted = new Map(difficult)
      reverted.delete(w)
      setDifficult(reverted)
    }
  }

  // Cache for word colors to avoid repeated DB queries
  const wordColorCache = useRef<Map<string, { EF: number; R: number; isNew: boolean }>>(new Map())

  /**
   * Tokenize text using Lindera and query DB for word colors.
   * For verbs/adjectives, uses dictionary form (base form) for DB lookup.
   * Returns both tokens and color map for consistent rendering.
   */
  async function getWordColors(text: string): Promise<{ tokens: Array<{surface: string, base_form: string}>, colors: Map<string, { EF: number; R: number; isNew: boolean }> }> {
    if (lang !== 'ja') {
      return { tokens: [], colors: new Map() }
    }

    try {
      // Tokenize using Lindera
      type TokenWithBase = { surface: string; base_form: string }
      const tokens = await invoke<TokenWithBase[]>('tokenize_japanese', { text })

      // Extract unique base forms (for verbs/adjectives, this is the dictionary form)
      const baseForms = [...new Set(tokens.map(t => t.base_form.toLowerCase()))]

      // Check cache first
      const cached = new Map<string, { EF: number; R: number; isNew: boolean }>()
      const uncached: string[] = []

      for (const base of baseForms) {
        if (wordColorCache.current.has(base)) {
          cached.set(base, wordColorCache.current.get(base)!)
        } else {
          uncached.push(base)
        }
      }

      // Query DB for uncached words
      if (uncached.length > 0) {
        const items = await invoke<ReviewItemRaw[]>('db_get_review_items_by_words', { words: uncached })
        const foundWords = new Set<string>()

        for (const item of items) {
          const info = { EF: item.EF, R: item.R, isNew: item.state === 'new' || !item.state }
          wordColorCache.current.set(item.content.toLowerCase(), info)
          cached.set(item.content.toLowerCase(), info)
          foundWords.add(item.content.toLowerCase())
        }
      }

      return { tokens, colors: cached }
    } catch (e) {
      console.error('Failed to get word colors:', e)
      return { tokens: [], colors: new Map() }
    }
  }

  function stripMarkdownFence(text: string): string {
    return text.replace(/^```markdown\s*\n?/i, '').replace(/\n?```\s*$/,'').trim()
  }

  async function handleGrammarAnalysis(messageId: number | null, messageContent: string) {
    if (!messageId) return
    if (grammarAnalysisLoading === messageId) return

    // Check cache
    try {
      const cached = await invoke<{ message_id: number; analysis: string } | null>('db_get_grammar_analysis', { messageId })
      if (cached) {
        setGrammarAnalysis(stripMarkdownFence(cached.analysis))
        return
      }
    } catch {}

    // No cache — call LLM
    if (!apiHost || !apiKey) return

    setGrammarAnalysisLoading(messageId)
    try {
      const analysis = await invoke<string>('db_analyze_grammar', {
        apiHost,
        apiKey,
        model,
        messageId,
        messageContent,
        language: lang || 'ja',
        grammarPatterns: selectedGrammarItems.map(g => g.pattern),
      })
      setGrammarAnalysis(stripMarkdownFence(analysis))
    } catch (e) {
      const errMsg = e instanceof Error ? e.message : String(e)
      setGrammarAnalysis(`**Error analyzing grammar:** ${errMsg}`)
    } finally {
      setGrammarAnalysisLoading(null)
    }
  }

  /**
   * Given annotated text (with {kanji|reading} syntax), a plain-text
   * character offset, and a length, extract the corresponding slice
   * of annotated text.
   *
   * Example: annotated = "abc{漢字|かんじ}def", offset=3, length=2
   *   → returns "{漢字|かんじ}" (plain chars at index 3,4 are 漢字)
   */
  function sliceAnnotated(annotated: string, plainOffset: number, plainLength: number): string {
    let plainIdx = 0
    let start = -1
    let end = -1

    for (let i = 0; i < annotated.length; ) {
      if (annotated[i] === '{') {
        // Find the closing }
        const closeIdx = annotated.indexOf('}', i)
        if (closeIdx === -1) break

        // Extract kanji part (before |)
        const inner = annotated.slice(i + 1, closeIdx)
        const pipeIdx = inner.indexOf('|')
        const kanji = pipeIdx >= 0 ? inner.slice(0, pipeIdx) : inner

        // Each kanji char counts as one plain char
        for (let k = 0; k < kanji.length; k++) {
          if (plainIdx === plainOffset && start === -1) {
            start = i
          }
          plainIdx++
          if (plainIdx === plainOffset + plainLength) {
            end = closeIdx + 1
            return annotated.slice(start, end)
          }
        }
        i = closeIdx + 1
      } else {
        if (plainIdx === plainOffset && start === -1) {
          start = i
        }
        plainIdx++
        if (plainIdx === plainOffset + plainLength) {
          end = i + 1
          return annotated.slice(start, end)
        }
        i++
      }
    }

    // Fallback: if we didn't find exact boundaries, return from start to end
    if (start !== -1) {
      return annotated.slice(start)
    }
    return ''
  }

  /**
   * Take up to `count` items from `arr` starting at `startIndex`, wrapping around.
   * Returns [selectedItems, nextStartIndex].
   */
  function dequeueRotate<T>(arr: T[], startIndex: number, count: number): [T[], number] {
    if (arr.length === 0) return [[], 0]
    const safeStart = startIndex % arr.length
    const take = Math.min(count, arr.length)
    const selected: T[] = []
    for (let i = 0; i < take; i++) {
      selected.push(arr[(safeStart + i) % arr.length])
    }
    const nextIndex = (safeStart + take) % arr.length
    return [selected, nextIndex]
  }

  function buildTargetWordsList(): string[] {
    const difficultArr = Array.from(difficult.keys())
    const knownArr = Array.from(known).filter(w => !difficult.has(w))
    // Shuffle known words so each rotation batch is random
    for (let i = knownArr.length - 1; i > 0; i--) {
      const j = Math.floor(Math.random() * (i + 1))
      ;[knownArr[i], knownArr[j]] = [knownArr[j], knownArr[i]]
    }
    const maxWords = getMaxDifficultWords()
    const maxKnown = getMaxKnownWords()

    // Pick difficult words that haven't been used yet in this rotation cycle.
    // When all difficult words have been used, reset and start a new cycle.
    const unusedDifficult = difficultArr.filter(w => !usedDifficultWords.current.has(w))
    let selectedDifficult: string[]
    if (unusedDifficult.length >= maxWords) {
      selectedDifficult = unusedDifficult.slice(0, maxWords)
    } else {
      // Not enough unused words left — take what's available, then reset and fill the rest
      selectedDifficult = [...unusedDifficult]
      usedDifficultWords.current.clear()
      const remaining = maxWords - selectedDifficult.length
      const freshPool = difficultArr.filter(w => !selectedDifficult.includes(w))
      selectedDifficult.push(...freshPool.slice(0, remaining))
    }
    // Mark selected words as used
    for (const w of selectedDifficult) {
      usedDifficultWords.current.add(w)
    }
    localStorage.setItem(WORD_QUEUE_USED_KEY, JSON.stringify([...usedDifficultWords.current]))

    const [selectedKnown, nextKnownIdx] = dequeueRotate(knownArr, knownQueueIndex.current, maxKnown)
    knownQueueIndex.current = nextKnownIdx
    return [...selectedDifficult, ...selectedKnown]
  }

  function buildSSRSystemPrompt(targetWords: string[]): string {
    const langName = langInfo?.label || 'the target language'

    let prompt = `You are a language learning conversation partner. The target language is ${langName}. Reply in ${langName} only. `
    prompt += `Keep the conversation natural and engaging. `

    if (lang === 'ja') {
      // JLPT level constraints
      const jlptInfo = JLPT_LEVELS[jlptLevel]
      if (jlptInfo) {
        prompt += `\n\n${jlptInfo.promptRule}`
      }
    }

    // Grammar practice instructions
    if (selectedGrammar.size > 0) {
      const patterns = selectedGrammarItems.map(g => g.pattern)
      prompt += `\n\nGRAMMAR PRACTICE RULE — YOU MUST FOLLOW THIS:\n`
      prompt += `The learner wants to practice these grammar patterns: ${patterns.join(', ')}.\n`
      prompt += `You MUST use ALL of them in your response. Weave them naturally into your sentences.\n`
    }

    // Split targetWords back into difficult vs known using the original maps
    const selectedDiff = targetWords.filter(w => difficult.has(w))
    const selectedKn = targetWords.filter(w => !difficult.has(w))

    if (selectedDiff.length > 0 || selectedKn.length > 0) {
      prompt += `\n\nCRITICAL SSR RULE — YOU MUST FOLLOW THIS:\n`
      if (selectedDiff.length > 0) {
        prompt += `- DIFFICULT words (you MUST use ALL of these in your response): ${selectedDiff.join(', ')}\n`
        prompt += `  These are words the learner is struggling with. Using them reinforces memory. Do NOT skip any.\n`
      }
      if (selectedKn.length > 0) {
        prompt += `- KNOWN words (use at least half of these to maintain retention): ${selectedKn.join(', ')}\n`
      }
      prompt += `\nWeave these naturally into your sentences. Do NOT list them explicitly. Each difficult word MUST appear at least once in your response.`
    }

    prompt += `\n\nIf the user seems stuck, offer a hint or rephrase. Encourage them to keep trying in ${langName}.`

    // Append user-configured custom prompt
    const customPrompt = getChatCustomPrompt()
    if (customPrompt.trim()) {
      prompt += `\n\nUSER CONFIGURATION:\n${customPrompt.trim()}`
    }

    return prompt
  }

  /**
   * Check if the user's message contains any of the target words.
   * For Japanese, uses simple includes check (Lindera base forms not easily accessible here).
   * For other languages, uses word boundary matching.
   */
  function checkTargetWordUsage(message: string, targetWords: string[]): boolean {
    if (lang === 'ja') {
      return targetWords.some(tw => message.toLowerCase().includes(tw.toLowerCase()))
    }
    const words = message.toLowerCase().split(/\s+/)
    return targetWords.some(tw => words.includes(tw.toLowerCase()))
  }

  async function send(message?: string) {
    const msg = message?.trim() || input.trim()
    if (!msg) return
    if (!sessionId) {
      try {
        const id = await invoke<string>('db_new_session', { title: null, language: lang || null, level: lang === 'ja' ? jlptLevel : null })
        setSessionId(id)
      } catch {}
    }
    // Capture target words BEFORE building prompt (queue advances in buildTargetWordsList)
    const targetWords = buildTargetWordsList()
    // Find parent for user message: last assistant message ID (or null for first message)
    const userParentId = [...visibleMessages].reverse().find(m => m.role === 'assistant' && m.id)?.id ?? null
    const user: Msg = { id: null, role: 'user', content: msg, parentMessageId: userParentId, isActivePath: true }
    const nextMessages = [...messages, user]
    setMessages(nextMessages)
    const userId = await invoke<number>('db_add_message_in_session', { sessionId, role: 'user', content: user.content, parentMessageId: userParentId }).catch(() => -1)
    if (userId >= 0) {
      setMessages(prev => prev.map((m, idx) => idx === nextMessages.length - 1 ? { ...m, id: userId } : m))
    }
    // Spawn feedback evaluation in parallel (non-blocking, runs alongside main LLM chat)
    // Skip auto-sent topic suggestions (e.g., "Talk about food.")
    if (userId >= 0 && apiHost && apiKey && !/^talk about /i.test(msg)) {
      setFeedbackMap(prev => new Map(prev).set(userId, { messageId: userId, score: 0, feedback: '', loading: true }))
      // Build context from previous messages (exclude the current one being evaluated)
      const { visibleMessages: feedbackVisible } = computeVisiblePath(nextMessages.slice(0, -1), branchSelection)
      const contextMsgs = feedbackVisible.map(m => ({ role: m.role as string, content: m.content }))
      invoke<UserFeedback>('db_evaluate_user_message', {
        apiHost, apiKey, model,
        messageId: userId,
        messageContent: msg,
        language: lang || '',
        level: lang === 'ja' ? jlptLevel : '',
        contextMessages: contextMsgs,
        targetWords,
      }).then(fb => {
        const userUsedAnyTarget = checkTargetWordUsage(msg, targetWords)
        const finalScore = (!userUsedAnyTarget && targetWords.length > 0) ? 0 : fb.score
        setFeedbackMap(prev => { const next = new Map(prev); next.set(userId, { ...fb, messageId: userId, score: finalScore, loading: false }); return next })
      }).catch(() => {
        setFeedbackMap(prev => { const next = new Map(prev); const existing = next.get(userId); if (existing?.loading) next.delete(userId); return next })
      })
    }
    setInput('')
    await invoke('db_record_activity', { activityType: 'chat' }).catch(() => {})
    setTimeout(() => {
      if (logRef.current) logRef.current.scrollTop = logRef.current.scrollHeight
    }, 0)

    if (apiHost && apiKey) {
      setLoading(true)
      // Mark LLM as pending so typing indicator persists across navigation
      if (sessionId) setLLMPending(sessionId)
      try {
        // Build SSR system prompt using the same target words
        const systemPrompt = buildSSRSystemPrompt(targetWords)

        // Send visible conversation history with system prompt
        const { visibleMessages: contextVisible } = computeVisiblePath(nextMessages, branchSelection)
        const llmMessages = [
          { role: 'system' as const, content: systemPrompt },
          ...contextVisible.map(m => ({ role: m.role as string, content: m.content })),
        ]

        const res = await invoke<string>('llm_chat', {
          apiHost,
          apiKey,
          model,
          messages: llmMessages,
        })

        // Add furigana annotations using lindera (only for Japanese)
        const annotatedRes = lang === 'ja'
          ? await invoke<string>('add_furigana', { text: res, language: lang })
          : res

        // Always save assistant response to DB so it persists across navigation
        // Parent of assistant message is the user message it responds to
        const assistantParentId = userId >= 0 ? userId : null
        const assistantId = await invoke<number>('db_add_message_in_session', { sessionId, role: 'assistant', content: annotatedRes, parentMessageId: assistantParentId }).catch(() => -1)

        // Only update React state if component is still mounted
        if (mounted.current) {
          const assistant: Msg = { id: null, role: 'assistant', content: annotatedRes, parentMessageId: assistantParentId, isActivePath: true }
          const targetIdx = nextMessages.length
          setMessages(prev => [...prev, assistant])
          if (assistantId >= 0) {
            setMessages(prev => prev.map((m, idx) => idx === targetIdx ? { ...m, id: assistantId } : m))
          }
        }
      } catch (e) {
        const errMsg = e instanceof Error ? e.message : String(e)

        // Always save error to DB for persistence
        const errorContent = `[Error] ${errMsg}`
        const errorParentId = userId >= 0 ? userId : null
        const errorId = await invoke<number>('db_add_message_in_session', { sessionId, role: 'assistant', content: errorContent, parentMessageId: errorParentId }).catch(() => -1)

        // Only update React state if component is still mounted
        if (mounted.current) {
          const errorMsg: Msg = { id: null, role: 'assistant', content: errorContent, parentMessageId: errorParentId, isActivePath: true }
          const errorIdx = nextMessages.length
          setMessages(prev => [...prev, errorMsg])
          if (errorId >= 0) {
            setMessages(prev => prev.map((m, idx) => idx === errorIdx ? { ...m, id: errorId } : m))
          }
        }
      } finally {
        if (sessionId) clearLLMPending(sessionId)
        if (mounted.current) setLoading(false)
      }
    } else {
      const fallback = `Let's talk about it. Which parts feel tricky for you? Try rephrasing in the target language.`
      const fallbackParentId = userId >= 0 ? userId : null
      const fallbackMsg: Msg = { id: null, role: 'assistant', content: fallback, parentMessageId: fallbackParentId, isActivePath: true }
      const fallbackIdx = nextMessages.length
      setMessages(prev => [...prev, fallbackMsg])
      const fallbackId = await invoke<number>('db_add_message_in_session', { sessionId, role: 'assistant', content: fallback, parentMessageId: fallbackParentId }).catch(() => -1)
      if (fallbackId >= 0) {
        setMessages(prev => prev.map((m, idx) => idx === fallbackIdx ? { ...m, id: fallbackId } : m))
      }
    }
  }

  async function handleEditSave(m: Msg) {
    if (!m.id || !input.trim()) return
    const newContent = input.trim()

    // 1. Deactivate old branch (old user msg + its bot response + descendants)
    await invoke('db_deactivate_message_branch', { messageId: m.id })

    // 1b. Update React state: mark deactivated messages as inactive
    // Collect all message IDs that should be deactivated (the edited msg + its descendants)
    const toDeactivate = new Set<number>()
    toDeactivate.add(m.id!)
    // Find children of the deactivated message in current state
    const findDescendants = (parentId: number) => {
      for (const msg of messages) {
        if (msg.parentMessageId === parentId && msg.id) {
          toDeactivate.add(msg.id)
          findDescendants(msg.id)
        }
      }
    }
    findDescendants(m.id!)
    setMessages(prev => prev.map(msg =>
      msg.id && toDeactivate.has(msg.id) ? { ...msg, isActivePath: false } : msg
    ))

    // 2. Find the parent of the old user message (for the tree link)
    const parentMsgId = m.parentMessageId ?? null

    // 3. Add the edited user message as a new active message in the tree
    const editedMsg: Msg = { id: null, role: 'user', content: newContent, parentMessageId: parentMsgId, isActivePath: true }
    setMessages(prev => [...prev, editedMsg])

    const editedId = await invoke<number>('db_add_message_in_session', {
      sessionId, role: 'user', content: newContent, parentMessageId: parentMsgId
    }).catch(() => -1)

    if (editedId >= 0) {
      setMessages(prev => prev.map((msg, idx) =>
        idx === prev.length - 1 ? { ...msg, id: editedId } : msg
      ))
      // Update branch selection to point to the new message
      setBranchSelection(prev => {
        const next = new Map(prev)
        next.set(parentMsgId, editedId)
        return next
      })
    }

    setEditingMessageId(null)
    setInput('')

    // 4. Re-trigger LLM chat + feedback
    setLoading(true)
    if (sessionId) setLLMPending(sessionId)

    const targetWords = buildTargetWordsList()
    const systemPrompt = buildSSRSystemPrompt(targetWords)

    // Build conversation history: visible path including the edited message
    const tempMessages = [...messages.filter(msg => !toDeactivate.has(msg.id ?? -1)), editedMsg]
    const { visibleMessages: contextVisible } = computeVisiblePath(tempMessages, branchSelection)
    const llmMessages = [
      { role: 'system' as const, content: systemPrompt },
      ...contextVisible.map(msg => ({ role: msg.role as string, content: msg.content })),
    ]

    try {
      const res = await invoke<string>('llm_chat', { apiHost, apiKey, model, messages: llmMessages })
      const annotatedRes = lang === 'ja'
        ? await invoke<string>('add_furigana', { text: res, language: lang })
        : res

      // Bot response's parent is the edited user message
      const assistantId = await invoke<number>('db_add_message_in_session', {
        sessionId, role: 'assistant', content: annotatedRes, parentMessageId: editedId >= 0 ? editedId : null
      }).catch(() => -1)

      if (mounted.current) {
        const assistant: Msg = { id: null, role: 'assistant', content: annotatedRes, parentMessageId: editedId >= 0 ? editedId : null, isActivePath: true }
        setMessages(prev => [...prev, assistant])
        if (assistantId >= 0) {
          setMessages(prev => prev.map((msg, idx) =>
            idx === prev.length - 1 ? { ...msg, id: assistantId } : msg
          ))
        }
      }

      // Re-evaluate feedback for the edited message
      if (editedId >= 0 && apiHost && apiKey) {
        const fbId = editedId
        setFeedbackMap(prev => new Map(prev).set(fbId, { messageId: fbId, score: 0, feedback: '', loading: true }))
        const contextMsgs = contextVisible.filter(m => m.id !== null).map(msg => ({ role: msg.role as string, content: msg.content }))
        invoke<UserFeedback>('db_evaluate_user_message', {
          apiHost, apiKey, model,
          messageId: fbId,
          messageContent: newContent,
          language: lang || '',
          level: lang === 'ja' ? jlptLevel : '',
          contextMessages: contextMsgs,
          targetWords,
        }).then(fb => {
          const userUsedAnyTarget = checkTargetWordUsage(newContent, targetWords)
          const finalScore = (!userUsedAnyTarget && targetWords.length > 0) ? 0 : fb.score
          setFeedbackMap(prev => {
            const next = new Map(prev)
            next.set(fbId, { ...fb, messageId: fbId, score: finalScore, loading: false })
            return next
          })
        }).catch(() => {
          setFeedbackMap(prev => {
            const next = new Map(prev)
            const existing = next.get(fbId)
            if (existing?.loading) next.delete(fbId)
            return next
          })
        })
      }
    } catch (e) {
      const errMsg = e instanceof Error ? e.message : String(e)
      const errorContent = `[Error] ${errMsg}`
      const errorId = await invoke<number>('db_add_message_in_session', {
        sessionId, role: 'assistant', content: errorContent, parentMessageId: editedId >= 0 ? editedId : null
      }).catch(() => -1)
      if (mounted.current) {
        const errorMsg: Msg = { id: null, role: 'assistant', content: errorContent, parentMessageId: editedId >= 0 ? editedId : null, isActivePath: true }
        setMessages(prev => [...prev, errorMsg])
        if (errorId >= 0) {
          setMessages(prev => prev.map((msg, idx) =>
            idx === prev.length - 1 ? { ...msg, id: errorId } : msg
          ))
        }
      }
    } finally {
      if (sessionId) clearLLMPending(sessionId)
      if (mounted.current) setLoading(false)
    }
  }

  function renderAssistantContent(content: string, messageId: number | null) {
    if (content.startsWith('[Error]')) {
      return <span className="text-red-500 text-sm">{content}</span>
    }
    return renderTextSegment(content, messageId)
  }

  function renderTextSegment(content: string, messageId: number | null) {
    const locale = lang === 'id' ? 'id' : 'ja'
    const isJapanese = lang === 'ja'

    // Get word colors and tokens for this message (fetched via Lindera tokenization)
    const messageData = messageId ? wordColorsByMessage.get(messageId) : undefined

    // For Japanese, use Lindera tokenization for consistency
    if (isJapanese && hasFuriganaAnnotations(content) && messageData) {
      const plain = stripFurigana(content)
      const { tokens, colors } = messageData

      // Walk through the annotated content alongside the Lindera tokens
      let annotIdx = 0
      const annotated = content

      return tokens.map((token, idx) => {
        const surface = token.surface
        const surfaceLower = surface.toLowerCase()
        const baseLower = token.base_form.toLowerCase()

        // Get color info using base form (dictionary form for verbs)
        const diffInfo = colors.get(baseLower)
        const isHard = !!diffInfo

        // Extract the annotated slice for this token
        const slice = sliceAnnotated(annotated, annotIdx, surface.length)
        annotIdx += surface.length

        // Check if this is a word (not punctuation/whitespace)
        const isWord = surface.trim().length > 0 && !/^[.,!?;:()"'""\[\]{}。、！？「」『』（）・\s]+$/.test(surface)

        if (!isWord) {
          return <span key={idx}>{renderFurigana(slice, showFurigana)}</span>
        }

        return (
          <span
            key={idx}
            onClick={() => markWordDifficult(surface)}
            className={`cursor-pointer rounded px-0.5 transition-colors ${
              isHard
                ? ''
                : 'hover:bg-blue-50 active:bg-yellow-50'
            }`}
            style={isHard ? { backgroundColor: difficultyColor(diffInfo.EF, diffInfo.R), color: '#fff' } : undefined}
            title={isHard ? 'Marked difficult' : 'Tap to mark difficult'}
          >
            {renderFurigana(slice, showFurigana)}
          </span>
        )
      })
    }

    // Fallback: Non-Japanese or no Lindera tokens available
    // Use splitTappable for rendering
    if (isJapanese && hasFuriganaAnnotations(content)) {
      const plain = stripFurigana(content)
      const parts = splitTappable(plain, 'ja')

      let annotIdx = 0
      const annotated = content

      return parts.map((part, idx) => {
        if (!isWordSegment(part)) {
          const slice = sliceAnnotated(annotated, annotIdx, part.length)
          annotIdx += part.length
          return <span key={idx}>{renderFurigana(slice, showFurigana)}</span>
        }

        const w = part.toLowerCase()
        const diffInfo = messageData?.colors.get(w)
        const isHard = !!diffInfo
        const slice = sliceAnnotated(annotated, annotIdx, part.length)
        annotIdx += part.length

        return (
          <span
            key={idx}
            onClick={() => markWordDifficult(part)}
            className={`cursor-pointer rounded px-0.5 transition-colors ${
              isHard
                ? ''
                : 'hover:bg-blue-50 active:bg-yellow-50'
            }`}
            style={isHard ? { backgroundColor: difficultyColor(diffInfo.EF, diffInfo.R), color: '#fff' } : undefined}
            title={isHard ? 'Marked difficult' : 'Tap to mark difficult'}
          >
            {renderFurigana(slice, showFurigana)}
          </span>
        )
      })
    }

    // Non-Japanese or no furigana annotations: standard word-tap rendering
    const parts = splitTappable(content, locale)
    return parts.map((part, idx) => {
      if (!isWordSegment(part)) return <span key={idx}>{part}</span>
      const w = part.toLowerCase()
      const diffInfo = messageData?.colors.get(w)
      const isHard = !!diffInfo

      return (
        <span
          key={idx}
          onClick={() => markWordDifficult(part)}
          className={`cursor-pointer rounded px-0.5 transition-colors ${
            isHard
              ? ''
              : 'hover:bg-blue-50 active:bg-yellow-50'
          }`}
          style={isHard ? { backgroundColor: difficultyColor(diffInfo.EF, diffInfo.R), color: '#fff' } : undefined}
          title={isHard ? 'Marked difficult' : 'Tap to mark difficult'}
        >
          {part}
        </span>
      )
    })
  }

  return (
    <div
      className="flex flex-col h-dvh bg-gray-50"
      style={{ height: `calc(100vh - ${keyboardInset}px)` }}
    >
      {/* Top bar */}
      <div className="sticky top-0 z-10 flex items-center gap-3 px-3 py-2 bg-white border-b shrink-0 safe-top">
        <button
          onClick={() => navigate('/chats')}
          className="flex items-center justify-center w-9 h-9 rounded-full hover:bg-gray-100 active:bg-gray-200 transition-colors"
        >
          <ArrowLeftOutlined className="text-lg" />
        </button>
        <div className="flex-1 min-w-0">
          <h2 className="text-base font-semibold truncate">
            {langInfo ? `${langInfo.flag} ${langInfo.label}` : 'Conversation'}
          </h2>
          <div className="flex items-center gap-1.5">
            {loading && (
              <span className="text-xs text-gray-400">typing...</span>
            )}
            {lang === 'ja' && (
              <span className="text-xs font-medium px-1.5 py-0.5 rounded bg-indigo-50 text-indigo-600">
                {jlptLevel}
              </span>
            )}
          </div>
        </div>
        {lang === 'ja' && (
          <button
            onClick={() => setShowFurigana(!showFurigana)}
            className={`shrink-0 px-2 py-1 rounded-full text-xs font-medium transition-colors ${
              showFurigana
                ? 'bg-blue-100 text-blue-700 hover:bg-blue-200'
                : 'bg-gray-100 text-gray-700 hover:bg-gray-200'
            }`}
          >
            {showFurigana ? 'ふりがな ON' : 'ふりがな OFF'}
          </button>
        )}
      </div>

      {/* Grammar selection row */}
      {lang === 'ja' && (
        <div className="shrink-0 bg-white border-b px-3 py-2 relative">
          <div className="flex items-center gap-2 flex-wrap">
            {selectedGrammarItems.length === 0 && (
              <span className="text-xs text-gray-400">Select grammar to practice</span>
            )}
            {selectedGrammarItems.map(g => (
              <span
                key={g.id}
                className="inline-flex items-center gap-1 px-2.5 py-1 rounded-full text-sm bg-indigo-50 text-indigo-600"
              >
                <button
                  onClick={() => setGrammarInfoItem(g)}
                  className="hover:text-indigo-800 transition-colors"
                  title="Grammar info"
                >
                  <InfoCircleOutlined style={{ fontSize: 12 }} />
                </button>
                <span>{g.pattern}</span>
                <button
                  onClick={() => {
                    const next = new Set(selectedGrammar)
                    next.delete(g.id)
                    setSelectedGrammar(next)
                  }}
                  className="hover:text-indigo-800 transition-colors"
                  title="Remove"
                >
                  <CloseOutlined style={{ fontSize: 10 }} />
                </button>
              </span>
            ))}
            <button
              data-grammar-btn
              onClick={() => setShowGrammarPicker(!showGrammarPicker)}
              className="shrink-0 px-2.5 py-1 rounded-full text-sm bg-indigo-50 text-indigo-600 hover:bg-indigo-100 active:bg-indigo-200 transition-colors flex items-center gap-1"
            >
              <PlusOutlined style={{ fontSize: 11 }} />
              Grammar
            </button>
          </div>

          {/* Grammar picker dropdown */}
          {showGrammarPicker && (
            <div
              data-grammar-picker
              className="absolute left-3 right-3 top-full mt-1 bg-white rounded-xl shadow-lg border border-gray-200 z-20 py-2 max-h-[50vh] overflow-y-auto"
            >
              <div className="px-2 space-y-0.5">
                {grammarItems.length === 0 && (
                  <div className="px-3 py-2 text-sm text-gray-400">No grammar items for this level</div>
                )}
                {grammarItems.map(g => {
                  const isSelected = selectedGrammar.has(g.id)
                  return (
                    <label
                      key={g.id}
                      className="flex items-center gap-2 px-3 py-2 rounded-lg text-sm hover:bg-gray-50 cursor-pointer transition-colors"
                    >
                      <input
                        type="checkbox"
                        checked={isSelected}
                        onChange={() => {
                          const next = new Set(selectedGrammar)
                          if (isSelected) next.delete(g.id)
                          else next.add(g.id)
                          setSelectedGrammar(next)
                        }}
                        className="accent-indigo-500"
                      />
                      <span className={isSelected ? 'text-indigo-600 font-medium' : 'text-gray-700'}>
                        {g.pattern}
                      </span>
                    </label>
                  )
                })}
              </div>
            </div>
          )}
        </div>
      )}

      {/* Grammar info modal */}
      {grammarInfoItem && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/30"
          onClick={() => setGrammarInfoItem(null)}
        >
          <div
            className="bg-white rounded-2xl p-5 mx-4 max-w-sm w-full max-h-[70vh] overflow-y-auto"
            onClick={e => e.stopPropagation()}
          >
            <div className="flex items-center justify-between mb-3">
              <h3 className="text-base font-semibold">{grammarInfoItem.pattern}</h3>
              <button
                onClick={() => setGrammarInfoItem(null)}
                className="text-gray-400 hover:text-gray-600"
              >
                <CloseOutlined />
              </button>
            </div>
            <div className="grammar-detail prose prose-sm max-w-none text-gray-600">
              <ReactMarkdown>{grammarInfoItem.detail}</ReactMarkdown>
            </div>
          </div>
        </div>
      )}

      {/* Grammar analysis modal */}
      {grammarAnalysis !== null && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/30"
          onClick={() => setGrammarAnalysis(null)}
        >
          <div
            className="bg-white rounded-2xl p-5 mx-4 max-w-sm w-full max-h-[70vh] overflow-y-auto"
            onClick={e => e.stopPropagation()}
          >
            <div className="flex items-center justify-between mb-3">
              <h3 className="text-base font-semibold">Grammar Analysis</h3>
              <button
                onClick={() => setGrammarAnalysis(null)}
                className="text-gray-400 hover:text-gray-600"
              >
                <CloseOutlined />
              </button>
            </div>
            <div className="grammar-detail prose prose-sm max-w-none text-gray-600">
              <ReactMarkdown>{grammarAnalysis}</ReactMarkdown>
            </div>
          </div>
        </div>
      )}

      {/* Messages area */}
      <div ref={logRef} className="flex-1 overflow-y-auto px-3 py-3">
        {messages.length === 0 && !loading && (
          <div className="flex flex-col items-center justify-end h-full text-gray-400 gap-3 pb-2">
            <div className="text-4xl">💬</div>
            <p className="text-sm text-center">Start a conversation in {langInfo?.label || 'your target language'}</p>
            <p className="text-xs text-center text-blue-400">Tap words in replies to mark them difficult</p>
            <div className="w-full mt-1 overflow-x-auto">
              <div className="grid gap-2" style={{ gridTemplateRows: 'repeat(3, auto)', gridAutoFlow: 'column' }}>
              {SUGGESTIONS.map((s, i) => (
                <button
                  key={s}
                  onClick={() => send(`Talk about ${s.toLowerCase()}.`)}
                  className="shrink-0 whitespace-nowrap px-4 py-2 rounded-full text-sm bg-white border border-gray-200 text-gray-600 shadow-sm hover:bg-blue-50 hover:border-blue-200 hover:text-blue-600 active:bg-blue-100 transition-all duration-200 animate-fade-in-up"
                  style={{ animationDelay: `${i * 80}ms` }}
                >
                  {s}
                </button>
              ))}
              </div>
            </div>
          </div>
        )}
        {visibleMessages.map((m, i) => {
          // Check if this message is at a branch point
          const parentKey = m.parentMessageId ?? null
          const branchPoint = branchPoints.get(parentKey)
          const isAtBranchPoint = !!branchPoint && branchPoint.siblings.length > 1

          // For user messages at a branch point, render with arrow navigation
          if (m.role === 'user' && isAtBranchPoint && branchPoint) {
            const { siblings, selectedIndex } = branchPoint
            const hasOlder = selectedIndex > 0
            const hasNewer = selectedIndex < siblings.length - 1
            const isActiveVersion = m.isActivePath !== false

            return (
              <div key={m.id ?? `unsaved-${i}`}>
                <div className="flex mb-2 justify-end flex-col items-end">
                  {/* Arrow navigation for branch siblings */}
                  <div className="flex items-center gap-1 mb-0.5">
                    <button
                      onClick={() => {
                        if (hasOlder) {
                          const olderSibling = siblings[selectedIndex - 1]
                          setBranchSelection(prev => new Map(prev).set(parentKey, olderSibling.id!))
                        }
                      }}
                      disabled={!hasOlder}
                      className={`text-[10px] px-1.5 py-0.5 rounded transition-colors ${
                        hasOlder ? 'text-blue-400 hover:text-blue-200' : 'text-gray-500 cursor-not-allowed'
                      }`}
                      title="Older version"
                    >
                      <RightOutlined style={{ fontSize: 9 }} />
                    </button>
                    <span className="text-[9px] text-blue-300/70">edited {selectedIndex + 1}/{siblings.length}</span>
                    <button
                      onClick={() => {
                        if (hasNewer) {
                          const newerSibling = siblings[selectedIndex + 1]
                          setBranchSelection(prev => new Map(prev).set(parentKey, newerSibling.id!))
                        }
                      }}
                      disabled={!hasNewer}
                      className={`text-[10px] px-1.5 py-0.5 rounded transition-colors ${
                        hasNewer ? 'text-blue-400 hover:text-blue-200' : 'text-gray-500 cursor-not-allowed'
                      }`}
                      title="Newer version"
                    >
                      <LeftOutlined style={{ fontSize: 9 }} />
                    </button>
                  </div>
                  <div
                    className={`max-w-[80%] px-3 py-2 text-[15px] leading-relaxed ${
                      isActiveVersion
                        ? 'bg-blue-500 text-white rounded-2xl rounded-br-md'
                        : 'bg-blue-300 text-white rounded-2xl rounded-br-md opacity-80'
                    }`}
                  >
                    {m.content}
                  </div>

                  {/* Edit button + Feedback badge */}
                  <div className="flex items-center gap-1">
                    {m.id && !loading && isActiveVersion && (
                      <button
                        onClick={() => {
                          setEditingMessageId(m.id)
                          setInput('')
                          setTimeout(() => inputRef.current?.focus(), 0)
                        }}
                        className="mt-1 text-blue-300 hover:text-blue-100 transition-colors"
                        title="Edit message"
                      >
                        <EditOutlined style={{ fontSize: 12 }} />
                      </button>
                    )}
                    <UserMessageFeedback feedback={m.id ? feedbackMap.get(m.id) : undefined} />
                  </div>
                </div>
              </div>
            )
          }

          // Normal message rendering (not at a branch point, or assistant message)
          return (
            <div key={m.id ?? `unsaved-${i}`} className={`flex mb-2 ${m.role === 'user' ? 'justify-end flex-col items-end' : 'justify-start'}`}>
              <div
                className={`
                  max-w-[80%] px-3 py-2 text-[15px] leading-relaxed
                  ${m.role === 'user'
                    ? 'bg-blue-500 text-white rounded-2xl rounded-br-md'
                    : 'bg-white text-gray-800 rounded-2xl rounded-bl-md shadow-sm'
                  }
                `}
              >
                {m.role === 'assistant' ? (
                  <div>
                    {renderAssistantContent(m.content, m.id)}
                    <button
                      onClick={() => handleGrammarAnalysis(m.id, m.content)}
                      disabled={m.id === null || grammarAnalysisLoading === m.id}
                      className="mt-1 text-gray-400 hover:text-indigo-500 transition-colors disabled:opacity-30"
                      title="Analyze grammar"
                    >
                      <InfoCircleOutlined style={{ fontSize: 14 }} />
                      {grammarAnalysisLoading === m.id && <span className="ml-1 text-xs text-gray-400">analyzing...</span>}
                    </button>
                  </div>
                ) : m.content}
              </div>

              {/* Edit button + Feedback badge for user messages */}
              {m.role === 'user' && (
                <div className="flex items-center gap-1">
                  {m.id && !loading && (
                    <button
                      onClick={() => {
                        setEditingMessageId(m.id)
                        setInput('')
                        setTimeout(() => inputRef.current?.focus(), 0)
                      }}
                      className="mt-1 text-blue-300 hover:text-blue-100 transition-colors"
                      title="Edit message"
                    >
                      <EditOutlined style={{ fontSize: 12 }} />
                    </button>
                  )}
                  <UserMessageFeedback feedback={m.id ? feedbackMap.get(m.id) : undefined} />
                </div>
              )}
            </div>
          )
        })}
        {loading && <TypingIndicator />}
      </div>

      {/* Bottom input bar */}
      <div
        className="shrink-0 bg-white border-t px-3 py-2 safe-bottom mb-1"
        style={{ paddingBottom: `calc(env(safe-area-inset-bottom, 0px) + ${keyboardInset}px)` }}
      >
        {editingMessageId && (
          <div className="flex items-center gap-2 mb-1">
            <span className="text-[10px] font-semibold text-blue-500 uppercase tracking-wider">Edit</span>
            <button
              onClick={() => { setEditingMessageId(null); setInput('') }}
              className="text-[10px] text-gray-400 hover:text-gray-600 transition-colors"
            >
              Cancel
            </button>
          </div>
        )}
        <div className="flex items-end gap-2">
          <TextArea
            ref={inputRef}
            value={input}
            onChange={(e) => setInput(e.target.value)}
            placeholder={editingMessageId ? 'Retype your message using target words...' : `Type in ${langInfo?.label || 'target language'}...`}
            className="flex-1 rounded-2xl bg-gray-100 border-none"
            style={{ borderRadius: 20, padding: '8px 16px', resize: 'none' }}
            autoSize={{ minRows: 1, maxRows: 4 }}
            disabled={loading}
            onPressEnter={(e) => {
              if (e.shiftKey) {
                e.preventDefault()
                if (editingMessageId) {
                  const editMsg = messages.find(m => m.id === editingMessageId)
                  if (editMsg) handleEditSave(editMsg)
                } else {
                  send()
                }
              }
            }}
          />
          <button
            onClick={() => {
              if (editingMessageId) {
                const editMsg = messages.find(m => m.id === editingMessageId)
                if (editMsg) handleEditSave(editMsg)
              } else {
                send()
              }
            }}
            disabled={!input.trim() || loading}
            className={`
              flex items-center justify-center w-10 h-10 rounded-full shrink-0 transition-colors
              ${input.trim() && !loading
                ? 'bg-blue-500 text-white hover:bg-blue-600 active:bg-blue-700'
                : 'bg-gray-100 text-gray-300'
              }
            `}
          >
            <SendOutlined />
          </button>
        </div>
      </div>
    </div>
  )
}
