import { useEffect, useMemo, useRef, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { splitTappable, isWordSegment } from '../lib/unknown'
import { renderFurigana, hasFuriganaAnnotations, stripFurigana, sliceAnnotated } from '../lib/furigana'
import { JLPT_LEVELS, DEFAULT_JLPT_LEVEL, JLPT_LEVEL_STORAGE_KEY } from '../lib/jlpt'
import type { JLPTLevel } from '../lib/jlpt'
import type { ReviewItem } from '../lib/types'
import { parseDetailJson } from '../lib/types'
import UserMessageFeedback, { type UserFeedback } from '../components/UserMessageFeedback'
import WordSheet, { keepSheetMeaning } from '../components/WordSheet'
import HelpReplySheet from '../components/HelpReplySheet'
import { t, lang as explainLang, errorText, languageName, partOfSpeechLabel } from '../lib/i18n'
import { languageLabel } from '../lib/languages'
import { readFlag, CHAT_HINTS_KEY, ROMAJI_KEY, ROMAJI_INPUT_KEY } from '../lib/prefs'
import { Input } from 'antd'
const { TextArea } = Input
import { ArrowLeftOutlined, SendOutlined, InfoCircleOutlined, CloseOutlined, PlusOutlined, EditOutlined, LeftOutlined, RightOutlined, DownOutlined, UpOutlined, TranslationOutlined, LoadingOutlined, BulbOutlined } from '@ant-design/icons'
import { useNavigate, useSearchParams } from 'react-router-dom'
import ReactMarkdown from 'react-markdown'

type Msg = { id: number | null, role: 'user' | 'assistant' | 'system', content: string, parentMessageId?: number | null, isActivePath?: boolean }

type BranchPointInfo = {
  siblings: Msg[]        // All sibling messages at this branch point
  selectedIndex: number  // Index of the currently selected sibling
}

/** Hint lines the partner may add (Settings → Hints in chat) start with this and are shown apart. */
const HINT_MARK = '💡'

/**
 * Topic presets. `prompt` is the English topic sent to the LLM and stored on the session;
 * the label shown is translated. The work-in-Japan topics come first for Indonesian learners.
 */
const SUGGESTIONS: { key: string; prompt: string }[] = [
  { key: 'random', prompt: 'Random Topic' },
  { key: 'selfIntro', prompt: 'Self-introduction' },
  { key: 'jobInterview', prompt: 'Job interview in Japan' },
  { key: 'atWork', prompt: 'At work in Japan (factory, elderly care, restaurant)' },
  { key: 'shopping', prompt: 'Shopping and the convenience store' },
  { key: 'station', prompt: 'At the train station' },
  { key: 'doctor', prompt: 'At the doctor' },
  { key: 'phone', prompt: 'Talking on the phone' },
  { key: 'travel', prompt: 'Travel' },
  { key: 'food', prompt: 'Food' },
  { key: 'work', prompt: 'Work' },
  { key: 'daily', prompt: 'Daily routine' },
  { key: 'hobbies', prompt: 'Hobbies' },
  { key: 'entertainment', prompt: 'Entertainment' },
  { key: 'technology', prompt: 'Technology' },
  { key: 'health', prompt: 'Health & Wellness' },
  { key: 'education', prompt: 'Learning & Education' },
  { key: 'relationships', prompt: 'Relationships' },
  { key: 'news', prompt: 'Current Events' },
  { key: 'nature', prompt: 'Nature & Environment' },
  { key: 'culture', prompt: 'Art & Culture' },
  { key: 'finance', prompt: 'Personal Finance' },
  { key: 'home', prompt: 'Home & Living' },
  { key: 'sports', prompt: 'Sports & Recreation' },
  { key: 'future', prompt: 'Future & Aspirations' },
]

/** Topic as shown to the learner (sessions store the English prompt). */
function topicLabel(topic: string): string {
  const s = SUGGESTIONS.find((x) => x.prompt === topic)
  return s ? t(`topic.${s.key}`) : topic
}

/** Splits the partner's reply into the target-language text and any hint lines. */
function splitHints(content: string): { body: string; hints: string[] } {
  const lines = content.split('\n')
  const hints = lines.filter((l) => l.trim().startsWith(HINT_MARK)).map((l) => l.trim().slice(HINT_MARK.length).trim())
  const body = lines.filter((l) => !l.trim().startsWith(HINT_MARK)).join('\n').trim()
  return { body, hints }
}

const MAX_REVIEWS_DEFAULT = 100
const MAX_REVIEWS_KEY = 'max_reviews_per_day'
const MAX_NEW_CARDS_DEFAULT = 20
const MAX_NEW_CARDS_KEY = 'max_new_cards_per_day'
const MAX_DIFFICULT_WORDS_KEY = 'max_difficult_words'
const MAX_DIFFICULT_WORDS_DEFAULT = 10
const MAX_KNOWN_WORDS_KEY = 'max_known_words'
const MAX_KNOWN_WORDS_DEFAULT = 20
const WORD_QUEUE_USED_KEY = 'word_queue_used_difficult'
const WORD_CACHE_KEY = 'word_classification_cache'
const WORD_CACHE_TTL_MS = 18 * 60 * 60 * 1000

type CachedWordClassification = {
  ts: number
  lang: string
  difficult: Array<{ content: string; ef: number; r: number; is_new: boolean; difficulty_bg_class: string }>
  known: string[]
}

function readWordCache(): CachedWordClassification | null {
  try {
    const raw = localStorage.getItem(WORD_CACHE_KEY)
    if (!raw) return null
    return JSON.parse(raw) as CachedWordClassification
  } catch {
    return null
  }
}

function writeWordCache(data: CachedWordClassification) {
  try {
    localStorage.setItem(WORD_CACHE_KEY, JSON.stringify(data))
  } catch {}
}

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

type GrammarDBItem = { id: string; pattern: string; detail: string; language: string; level: string; is_default: number; ai_translated?: boolean }

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

type WordColorInfo = { EF: number; R: number; isNew: boolean; difficulty_bg_class: string }

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
  const [difficult, setDifficult] = useState<Map<string, WordColorInfo>>(new Map())
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
  const [wordColorsByMessage, setWordColorsByMessage] = useState<Map<number, { tokens: Array<{surface: string, base_form: string}>, colors: Map<string, WordColorInfo> }>>(new Map())
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
  const [selectedTopic, setSelectedTopic] = useState<string | null>(null)
  const [topicVisible, setTopicVisible] = useState(true)
  const [atBottom, setAtBottom] = useState(true)
  const lastScrollTopRef = useRef(0)
  const [previewTargets, setPreviewTargets] = useState<{ difficult: string[]; known: string[]; resetUsed: boolean }>({ difficult: [], known: [], resetUsed: false })
  // Learning aids: tap-a-word sheet, translations, romaji, help me reply
  const [wordSheet, setWordSheet] = useState<{ word: string; base: string } | null>(null)
  const [translations, setTranslations] = useState<Map<number, string>>(new Map())
  const [shownTranslations, setShownTranslations] = useState<Set<number>>(new Set())
  const [translatingId, setTranslatingId] = useState<number | null>(null)
  const [romajiByMessage, setRomajiByMessage] = useState<Map<number, string>>(new Map())
  const [showHelpReply, setShowHelpReply] = useState(false)
  const romajiEnabled = lang === 'ja' && readFlag(ROMAJI_KEY)
  const romajiInput = lang === 'ja' && readFlag(ROMAJI_INPUT_KEY)
  const romajiInputSeq = useRef(0)

  // Detail popup for target chips
  const [detailIndex, setDetailIndex] = useState<number | null>(null)
  const [detailVisible, setDetailVisible] = useState(false)
  const [detailItems, setDetailItems] = useState<Map<string, ReviewItem>>(new Map())
  const detailScrollRef = useRef<HTMLDivElement>(null)
  const detailCardRefs = useRef<Map<number, HTMLDivElement>>(new Map())
  const detailCloseTimer = useRef<number | null>(null)

  const targetChips = useMemo(() => [
    ...previewTargets.difficult.map(w => ({ word: w, kind: 'hard' as const })),
    ...previewTargets.known.map(w => ({ word: w, kind: 'known' as const })),
  ], [previewTargets.difficult, previewTargets.known])

  const closeDetail = () => {
    setDetailVisible(false)
    if (detailCloseTimer.current) window.clearTimeout(detailCloseTimer.current)
    detailCloseTimer.current = window.setTimeout(() => {
      setDetailIndex(null)
      detailCloseTimer.current = null
    }, 220)
  }

  const openDetail = async (idx: number) => {
    if (detailCloseTimer.current) {
      window.clearTimeout(detailCloseTimer.current)
      detailCloseTimer.current = null
    }
    setDetailIndex(idx)
    // Defer visibility flip to the next frame so the enter transition runs
    requestAnimationFrame(() => {
      if (mounted.current) setDetailVisible(true)
    })
    const allWords = [...previewTargets.difficult, ...previewTargets.known]
    const missing = allWords.filter(w => !detailItems.has(w))
    if (missing.length === 0) return
    try {
      const items = await invoke<ReviewItem[]>('db_get_review_items_by_words', { words: missing })
      if (!mounted.current) return
      setDetailItems(prev => {
        const next = new Map(prev)
        for (const it of items) next.set(it.content, it)
        return next
      })
    } catch (e) {
      console.error('Failed to load word details', e)
    }
  }

  // Scroll the popup to the requested card whenever it (re)opens
  useEffect(() => {
    if (detailIndex == null) return
    const node = detailCardRefs.current.get(detailIndex)
    if (node) node.scrollIntoView({ behavior: 'instant' as ScrollBehavior, inline: 'center', block: 'nearest' })
  }, [detailIndex])

  // Track which card is currently centered as the user swipes
  const onDetailScroll = () => {
    const container = detailScrollRef.current
    if (!container) return
    const center = container.scrollLeft + container.clientWidth / 2
    let bestIdx = detailIndex ?? 0
    let bestDist = Infinity
    detailCardRefs.current.forEach((el, idx) => {
      const cardCenter = el.offsetLeft + el.offsetWidth / 2
      const dist = Math.abs(cardCenter - center)
      if (dist < bestDist) { bestDist = dist; bestIdx = idx }
    })
    if (bestIdx !== detailIndex) setDetailIndex(bestIdx)
  }

  // Close popup on Escape
  useEffect(() => {
    if (detailIndex == null) return
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') closeDetail() }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [detailIndex])

  const langName = languageName(lang)

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

  // Load session messages + topic
  useEffect(() => {
    async function load() {
      if (!sessionId) return
      try {
        invoke<string | null>('db_get_session_topic', { sessionId })
          .then(t => setSelectedTopic(t))
          .catch(() => {})
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

  // Load difficult/known words from backend (pre-classified).
  // Cached in localStorage for 18h to avoid hitting the DB on every chat open.
  useEffect(() => {
    async function load() {
      try {
        const cacheLang = lang || ''
        const cached = readWordCache()
        let raw: CachedWordClassification['difficult'] = []
        let knownArr: string[] = []
        const fresh = cached && cached.lang === cacheLang && Date.now() - cached.ts < WORD_CACHE_TTL_MS
        if (fresh) {
          raw = cached!.difficult
          knownArr = cached!.known
        } else {
          const data = await invoke<{ difficult: CachedWordClassification['difficult']; known: string[] }>(
            'db_get_difficult_and_known_words',
            { language: lang || null }
          )
          raw = data.difficult
          knownArr = data.known
          writeWordCache({ ts: Date.now(), lang: cacheLang, difficult: raw, known: knownArr })
        }
        const hardMap = new Map<string, WordColorInfo>()
        raw.forEach((w) => {
          hardMap.set(w.content.toLowerCase(), {
            EF: w.ef,
            R: w.r,
            isNew: w.is_new,
            difficulty_bg_class: w.difficulty_bg_class,
          })
        })
        setDifficult(hardMap)
        setKnown(new Set(knownArr.map((w) => w.toLowerCase())))
      } catch {
        console.error('Failed to load difficult/known words')
      }
    }
    load()
  }, [lang])

  // Fetch word colors for assistant messages using Lindera tokenization
  useEffect(() => {
    if (lang !== 'ja') return

    async function fetchColors() {
      const assistantMessages = messages.filter(m => m.role === 'assistant' && m.id && !wordColorsByMessage.has(m.id))
      if (assistantMessages.length === 0) return

      for (const msg of assistantMessages) {
        if (!msg.id) continue

        // Strip furigana for tokenization; hint lines are rendered apart, so leave them out
        const plainText = stripFurigana(splitHints(msg.content).body)
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

  // Romaji under each partner reply, for beginners (Settings → Show romaji)
  useEffect(() => {
    if (!romajiEnabled) return
    const pending = messages.filter(m => m.role === 'assistant' && m.id && !romajiByMessage.has(m.id) && !m.content.startsWith('[Error]'))
    for (const m of pending) {
      invoke<string>('to_romaji', { text: stripFurigana(splitHints(m.content).body) })
        .then(r => setRomajiByMessage(prev => new Map(prev).set(m.id!, r)))
        .catch(() => {})
    }
  }, [messages, romajiEnabled])

  async function toggleTranslation(m: Msg) {
    if (!m.id) return
    const id = m.id
    if (shownTranslations.has(id)) {
      setShownTranslations(prev => { const next = new Set(prev); next.delete(id); return next })
      return
    }
    setShownTranslations(prev => new Set(prev).add(id))
    if (translations.has(id) || !apiHost || !apiKey) return
    setTranslatingId(id)
    try {
      const text = await invoke<string>('llm_translate', {
        apiHost, apiKey, model, text: splitHints(m.content).body, fromLang: lang || 'ja', kind: 'message', refId: id,
      })
      setTranslations(prev => new Map(prev).set(id, text))
    } catch (e) {
      setTranslations(prev => new Map(prev).set(id, errorText(e)))
    }
    setTranslatingId(null)
  }

  // Typing romaji turns it into kana (for keyboards without Japanese). Only the latest call applies.
  function handleInputChange(value: string) {
    setInput(value)
    if (!romajiInput) return
    const seq = ++romajiInputSeq.current
    invoke<string>('romaji_to_kana_input', { text: value, finish: false })
      .then(kana => { if (seq === romajiInputSeq.current && kana !== value) setInput(kana) })
      .catch(() => {})
  }

  async function finishRomaji(text: string): Promise<string> {
    if (!romajiInput) return text
    return invoke<string>('romaji_to_kana_input', { text, finish: true }).catch(() => text)
  }

  // Adds a tapped word to the deck, keeping a meaning the sheet already looked up.
  async function addWordFromSheet(content: string, detail: string | null) {
    await markWordDifficult(content)
    await keepSheetMeaning(content, detail)
  }

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

  // Seed the preview chip strip when difficult/known sets load or change size.
  // Skipped if preview is already populated by a recent send (refs may have advanced).
  useEffect(() => {
    if (difficult.size === 0 && known.size === 0) return
    if (previewTargets.difficult.length === 0 && previewTargets.known.length === 0) {
      setPreviewTargets(pickTargetWords())
    }
  }, [difficult.size, known.size])

  // Auto-hide topic pill on scroll-down, reveal on scroll-up
  useEffect(() => {
    const el = logRef.current
    if (!el) return
    function onScroll() {
      if (!el) return
      const current = el.scrollTop
      const delta = current - lastScrollTopRef.current
      const scrollable = el.scrollHeight - el.clientHeight > 20
      const distFromBottom = el.scrollHeight - el.clientHeight - current
      if (!scrollable || current <= 0) {
        setTopicVisible(true)
      } else if (delta > 4) {
        setTopicVisible(false)
      } else if (delta < -4) {
        setTopicVisible(true)
      }
      setAtBottom(!scrollable || distFromBottom < 60)
      lastScrollTopRef.current = current
    }
    el.addEventListener('scroll', onScroll, { passive: true })
    return () => el.removeEventListener('scroll', onScroll)
  }, [])

  async function markWordDifficult(word: string) {
    const w = word.toLowerCase()
    const next = new Map(difficult)
    next.set(w, { EF: 2.5, R: 0, isNew: true, difficulty_bg_class: 'bg-red-500' })
    setDifficult(next)
    try {
      await invoke('db_upsert_review_item', { content: w, language: lang || null })
      // Write-through to cache so the new word survives within the 18h TTL.
      const cached = readWordCache()
      const cacheLang = lang || ''
      if (cached && cached.lang === cacheLang) {
        if (!cached.difficult.some(d => d.content.toLowerCase() === w)) {
          cached.difficult.unshift({ content: w, ef: 2.5, r: 0, is_new: true, difficulty_bg_class: 'bg-red-500' })
          writeWordCache(cached)
        }
      }
    } catch (e) {
      console.error('Failed to save difficult word to DB:', e)
      // Roll back local state if DB write fails
      const reverted = new Map(difficult)
      reverted.delete(w)
      setDifficult(reverted)
    }
  }

  // Cache for word colors to avoid repeated DB queries
  const wordColorCache = useRef<Map<string, WordColorInfo>>(new Map())

  /**
   * Tokenize text using Lindera and query DB for word colors.
   * For verbs/adjectives, uses dictionary form (base form) for DB lookup.
   * Returns both tokens and color map for consistent rendering.
   */
  async function getWordColors(text: string): Promise<{ tokens: Array<{surface: string, base_form: string}>, colors: Map<string, WordColorInfo> }> {
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
      const cached = new Map<string, WordColorInfo>()
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
        const items = await invoke<ReviewItem[]>('db_get_review_items_by_words', { words: uncached })
        const foundWords = new Set<string>()

        for (const item of items) {
          const info: WordColorInfo = {
            EF: item.EF,
            R: item.R,
            isNew: item.state === 'new' || !item.state,
            difficulty_bg_class: item.difficulty_bg_class,
          }
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
      setGrammarAnalysis(`**${t('chat.analysisFailed')}** ${errorText(errMsg)}`)
    } finally {
      setGrammarAnalysisLoading(null)
    }
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

  // Pure: choose target words without mutating rotation state.
  // Returns `resetUsed` so commit can replay the same reset decision.
  function pickTargetWords(): { difficult: string[]; known: string[]; resetUsed: boolean } {
    const difficultArr = Array.from(difficult.keys())
    const knownArr = Array.from(known).filter(w => !difficult.has(w))
    for (let i = knownArr.length - 1; i > 0; i--) {
      const j = Math.floor(Math.random() * (i + 1))
      ;[knownArr[i], knownArr[j]] = [knownArr[j], knownArr[i]]
    }
    const maxWords = getMaxDifficultWords()
    const maxKnown = getMaxKnownWords()

    const unusedDifficult = difficultArr.filter(w => !usedDifficultWords.current.has(w))
    let selectedDifficult: string[]
    let resetUsed = false
    if (unusedDifficult.length >= maxWords) {
      selectedDifficult = unusedDifficult.slice(0, maxWords)
    } else {
      selectedDifficult = [...unusedDifficult]
      resetUsed = true
      const remaining = maxWords - selectedDifficult.length
      const freshPool = difficultArr.filter(w => !selectedDifficult.includes(w))
      selectedDifficult.push(...freshPool.slice(0, remaining))
    }

    const [selectedKnown] = dequeueRotate(knownArr, knownQueueIndex.current, maxKnown)
    return { difficult: selectedDifficult, known: selectedKnown, resetUsed }
  }

  // Mutate rotation state to reflect a previously made pick.
  function commitTargetPick(picked: { difficult: string[]; known: string[]; resetUsed: boolean }) {
    if (picked.resetUsed) usedDifficultWords.current.clear()
    for (const w of picked.difficult) usedDifficultWords.current.add(w)
    localStorage.setItem(WORD_QUEUE_USED_KEY, JSON.stringify([...usedDifficultWords.current]))
    const knownArr = Array.from(known).filter(w => !difficult.has(w))
    if (knownArr.length > 0) {
      knownQueueIndex.current = (knownQueueIndex.current + picked.known.length) % knownArr.length
    }
  }

  // Send-time consumer: prefer the previewed pick so what user saw == what LLM gets.
  // Commits the pick and refreshes the preview for the next turn.
  function buildTargetWordsList(): string[] {
    const usePreview = previewTargets.difficult.length > 0 || previewTargets.known.length > 0
    const picked = usePreview ? previewTargets : pickTargetWords()
    commitTargetPick(picked)
    setPreviewTargets(pickTargetWords())
    return [...picked.difficult, ...picked.known]
  }

  function buildSSRSystemPrompt(targetWords: string[], topic?: string | null): string {
    // LLM-facing: always the English language name.
    const langName = ({ ja: 'Japanese', id: 'Indonesian' } as Record<string, string>)[lang] || 'the target language'

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

    if (topic) {
      const topicText = topic === 'Random Topic' ? 'a random interesting topic of your choice' : `"${topic}"`
      prompt += `\n\nCONVERSATION TOPIC: The topic for this session is ${topicText}. Keep the conversation centered around this topic.`
    }

    // One short hint in the learner's own language (Settings → Hints in chat), shown apart from the reply.
    if (readFlag(CHAT_HINTS_KEY)) {
      const explainName = ({ en: 'English', id: 'Indonesian (Bahasa Indonesia)' } as Record<string, string>)[explainLang]
      prompt += `\n\nHINT RULE: After your reply, add ONE extra line that starts with "${HINT_MARK} " and explains, in ${explainName}, one word or phrase from your reply the learner may not know (e.g. "${HINT_MARK} 飲み物 = ${explainLang === 'id' ? 'minuman' : 'drink'}"). Keep the hint under 12 words. Everything else stays in ${langName}.`
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
    const msg = (await finishRomaji(message?.trim() || input.trim())).trim()
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
        // Evaluation invoke failed — Rust may have still written to DB (async IPC drop).
        // Load from DB before giving up so the chip doesn't vanish.
        invoke<UserFeedback | null>('db_get_user_feedback', { messageId: userId })
          .then(fb => {
            if (fb) {
              setFeedbackMap(prev => new Map(prev).set(userId, { ...fb, messageId: userId, loading: false }))
            } else {
              setFeedbackMap(prev => { const next = new Map(prev); if (next.get(userId)?.loading) next.delete(userId); return next })
            }
          })
          .catch(() => {
            setFeedbackMap(prev => { const next = new Map(prev); if (next.get(userId)?.loading) next.delete(userId); return next })
          })
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
        const systemPrompt = buildSSRSystemPrompt(targetWords, selectedTopic)

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
      const fallback = t('chat.noApiFallback')
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

  async function startTopicConversation(topic: string) {
    setSelectedTopic(topic)
    if (!apiHost || !apiKey) return

    let sid = sessionId
    if (!sid) {
      try {
        sid = await invoke<string>('db_new_session', { title: topic, language: lang || null, level: lang === 'ja' ? jlptLevel : null })
        setSessionId(sid)
      } catch { return }
    }
    if (sid) invoke('db_set_session_topic', { sessionId: sid, topic }).catch(() => {})

    const targetWords = buildTargetWordsList()
    const systemPrompt = buildSSRSystemPrompt(targetWords, topic)

    setLoading(true)
    if (sid) setLLMPending(sid)
    await invoke('db_record_activity', { activityType: 'chat' }).catch(() => {})

    try {
      const topicInstruction = topic === 'Random Topic'
        ? 'Begin the conversation with a natural opening about an interesting topic of your choice.'
        : `Begin the conversation with a natural opening related to "${topic}".`

      const llmMessages = [
        { role: 'system' as const, content: systemPrompt },
        { role: 'user' as const, content: topicInstruction },
      ]

      const res = await invoke<string>('llm_chat', { apiHost, apiKey, model, messages: llmMessages })
      const annotatedRes = lang === 'ja'
        ? await invoke<string>('add_furigana', { text: res, language: lang })
        : res

      const assistantId = await invoke<number>('db_add_message_in_session', {
        sessionId: sid, role: 'assistant', content: annotatedRes, parentMessageId: null
      }).catch(() => -1)

      if (mounted.current) {
        const assistant: Msg = { id: null, role: 'assistant', content: annotatedRes, parentMessageId: null, isActivePath: true }
        setMessages(prev => [...prev, assistant])
        if (assistantId >= 0) {
          setMessages(prev => prev.map((m, idx) => idx === prev.length - 1 ? { ...m, id: assistantId } : m))
        }
      }
    } catch (e) {
      const errMsg = e instanceof Error ? e.message : String(e)
      const errorContent = `[Error] ${errMsg}`
      const errorId = await invoke<number>('db_add_message_in_session', {
        sessionId: sid, role: 'assistant', content: errorContent, parentMessageId: null
      }).catch(() => -1)
      if (mounted.current) {
        const errorMsg: Msg = { id: null, role: 'assistant', content: errorContent, parentMessageId: null, isActivePath: true }
        setMessages(prev => [...prev, errorMsg])
        if (errorId >= 0) {
          setMessages(prev => prev.map((m, idx) => idx === prev.length - 1 ? { ...m, id: errorId } : m))
        }
      }
    } finally {
      if (sid) clearLLMPending(sid)
      if (mounted.current) setLoading(false)
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
    const systemPrompt = buildSSRSystemPrompt(targetWords, selectedTopic)

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
          invoke<UserFeedback | null>('db_get_user_feedback', { messageId: fbId })
            .then(fb => {
              if (fb) {
                setFeedbackMap(prev => new Map(prev).set(fbId, { ...fb, messageId: fbId, loading: false }))
              } else {
                setFeedbackMap(prev => { const next = new Map(prev); if (next.get(fbId)?.loading) next.delete(fbId); return next })
              }
            })
            .catch(() => {
              setFeedbackMap(prev => { const next = new Map(prev); if (next.get(fbId)?.loading) next.delete(fbId); return next })
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
      return <span className="text-red-500 text-sm">{errorText(content.slice('[Error]'.length).trim())}</span>
    }
    const { body, hints } = splitHints(content)
    return (
      <>
        {renderTextSegment(body, messageId)}
        {romajiEnabled && messageId && romajiByMessage.get(messageId) && (
          <div className="text-xs text-gray-400 mt-1">{romajiByMessage.get(messageId)}</div>
        )}
        {hints.map((h, i) => (
          <div key={i} className="text-xs text-indigo-600 mt-1 flex items-start gap-1"><BulbOutlined className="mt-0.5" />{h}</div>
        ))}
      </>
    )
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
            onClick={() => setWordSheet({ word: surface, base: token.base_form || surface })}
            className={`cursor-pointer rounded px-0.5 transition-colors ${
              isHard
                ? diffInfo.difficulty_bg_class + ' text-white'
                : 'hover:bg-blue-50 active:bg-yellow-50'
            }`}
            style={isHard ? { color: '#fff' } : undefined}
            title={isHard ? t('chat.markedDifficult') : t('chat.tapForMeaning')}
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
            onClick={() => setWordSheet({ word: part, base: part })}
            className={`cursor-pointer rounded px-0.5 transition-colors ${
              isHard
                ? diffInfo.difficulty_bg_class + ' text-white'
                : 'hover:bg-blue-50 active:bg-yellow-50'
            }`}
            style={isHard ? { color: '#fff' } : undefined}
            title={isHard ? t('chat.markedDifficult') : t('chat.tapForMeaning')}
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
          onClick={() => setWordSheet({ word: part, base: part })}
          className={`cursor-pointer rounded px-0.5 transition-colors ${
            isHard
              ? diffInfo.difficulty_bg_class + ' text-white'
              : 'hover:bg-blue-50 active:bg-yellow-50'
          }`}
          style={isHard ? { color: '#fff' } : undefined}
          title={isHard ? t('chat.markedDifficult') : t('chat.tapForMeaning')}
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
          aria-label={t('common.back')}
          className="flex items-center justify-center w-9 h-9 rounded-full hover:bg-gray-100 active:bg-gray-200 transition-colors"
        >
          <ArrowLeftOutlined className="text-lg" />
        </button>
        <div className="flex-1 min-w-0">
          <h2 className="text-base font-semibold truncate">
            {lang ? languageLabel(lang) : t('chat.conversation')}
          </h2>
          <div className="flex items-center gap-1.5">
            {loading && (
              <span className="text-xs text-gray-400">{t('chat.typing')}</span>
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
            {showFurigana ? t('chat.furiganaOn') : t('chat.furiganaOff')}
          </button>
        )}
      </div>

      {/* Grammar selection row */}
      {lang === 'ja' && (
        <div className="shrink-0 bg-white border-b px-3 py-2 relative">
          <div className="flex items-center gap-2 flex-wrap">
            {selectedGrammarItems.length === 0 && (
              <span className="text-xs text-gray-400">{t('chat.selectGrammar')}</span>
            )}
            {selectedGrammarItems.map(g => (
              <span
                key={g.id}
                className="inline-flex items-center gap-1 px-2.5 py-1 rounded-full text-sm bg-indigo-50 text-indigo-600"
              >
                <button
                  onClick={() => setGrammarInfoItem(g)}
                  className="hover:text-indigo-800 transition-colors"
                  title={t('chat.grammarInfo')}
                  aria-label={t('chat.grammarInfo')}
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
                  title={t('chat.remove')}
                  aria-label={t('chat.remove')}
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
              {t('home.grammar')}
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
                  <div className="px-3 py-2 text-sm text-gray-400">{t('grammar.empty')}</div>
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
                aria-label={t('common.close')}
                className="text-gray-400 hover:text-gray-600"
              >
                <CloseOutlined />
              </button>
            </div>
            <div className="grammar-detail prose prose-sm max-w-none text-gray-600">
              <ReactMarkdown>{grammarInfoItem.detail}</ReactMarkdown>
            </div>
            {grammarInfoItem.ai_translated && <div className="text-[11px] text-indigo-600 mt-2">{t('common.aiTranslated')}</div>}
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
              <h3 className="text-base font-semibold">{t('chat.grammarAnalysis')}</h3>
              <button
                onClick={() => setGrammarAnalysis(null)}
                aria-label={t('common.close')}
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
      <div ref={logRef} className="flex-1 overflow-y-auto px-3 py-3 relative">
        {/* Floating topic badge */}
        {selectedTopic && (
          <div
            className={`sticky top-0 z-10 flex justify-center pb-2 transition-all duration-300 ease-out pointer-events-none ${
              topicVisible
                ? 'opacity-100 translate-y-0'
                : 'opacity-0 -translate-y-2'
            }`}
          >
            <span className="inline-flex items-center gap-1.5 px-3 py-1 rounded-full bg-amber-50/95 border border-amber-200 text-xs font-medium text-amber-700 shadow-sm backdrop-blur-sm pointer-events-auto">
              <span className="text-[10px]">💬</span>
              <span className="text-[10px] text-amber-400 uppercase tracking-wider">{t('chat.topic')}</span>
              {topicLabel(selectedTopic)}
            </span>
          </div>
        )}
        {messages.length === 0 && !loading && (
          <div className="flex flex-col items-center justify-end h-full text-gray-400 gap-3 pb-2">
            <div className="text-4xl">💬</div>
            <p className="text-sm text-center">{t('chat.startIn', { language: langName || t('chat.yourTarget') })}</p>
            <p className="text-xs text-center text-blue-400">{t('chat.tapWordsHint')}</p>
            <div className="w-full mt-1 overflow-x-auto">
              <div className="grid gap-2" style={{ gridTemplateRows: 'repeat(3, auto)', gridAutoFlow: 'column' }}>
              {SUGGESTIONS.map((s, i) => (
                <button
                  key={s.key}
                  onClick={() => startTopicConversation(s.prompt)}
                  className="shrink-0 whitespace-nowrap px-4 py-2 rounded-full text-sm bg-white border border-gray-200 text-gray-600 shadow-sm hover:bg-blue-50 hover:border-blue-200 hover:text-blue-600 active:bg-blue-100 transition-all duration-200 animate-fade-in-up"
                  style={{ animationDelay: `${i * 80}ms` }}
                >
                  {t(`topic.${s.key}`)}
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
                      title={t('chat.olderVersion')}
                      aria-label={t('chat.olderVersion')}
                    >
                      <RightOutlined style={{ fontSize: 9 }} />
                    </button>
                    <span className="text-[9px] text-blue-300/70">{t('chat.edited', { n: selectedIndex + 1, total: siblings.length })}</span>
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
                      title={t('chat.newerVersion')}
                      aria-label={t('chat.newerVersion')}
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
                        title={t('chat.editMessage')}
                        aria-label={t('chat.editMessage')}
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
                    {m.id && shownTranslations.has(m.id) && (
                      <div className="mt-2 pt-2 border-t border-dashed border-gray-200 text-sm text-gray-600 whitespace-pre-wrap">
                        {translatingId === m.id ? <LoadingOutlined spin /> : translations.get(m.id)}
                      </div>
                    )}
                    {!m.content.startsWith('[Error]') && (
                      <div className="flex items-center gap-3 mt-1">
                        <button
                          onClick={() => toggleTranslation(m)}
                          disabled={m.id === null || !apiHost || !apiKey}
                          className="flex items-center gap-1 text-xs text-gray-400 hover:text-blue-500 transition-colors disabled:opacity-30"
                        >
                          <TranslationOutlined style={{ fontSize: 12 }} />
                          {m.id && shownTranslations.has(m.id) ? t('chat.hideTranslation') : t('chat.translate')}
                        </button>
                        <button
                          onClick={() => handleGrammarAnalysis(m.id, m.content)}
                          disabled={m.id === null || grammarAnalysisLoading === m.id}
                          className="flex items-center gap-1 text-xs text-gray-400 hover:text-indigo-500 transition-colors disabled:opacity-30"
                          title={t('chat.analyzeGrammar')}
                        >
                          <InfoCircleOutlined style={{ fontSize: 12 }} />
                          {grammarAnalysisLoading === m.id ? t('chat.analyzing') : t('home.grammar')}
                        </button>
                      </div>
                    )}
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
                      title={t('chat.editMessage')}
                        aria-label={t('chat.editMessage')}
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

      {/* Target word chip strip — visible only on user's turn at the bottom of the scroll */}
      {(() => {
        const hasTargets = previewTargets.difficult.length > 0 || previewTargets.known.length > 0
        const visible = hasTargets && !loading && atBottom && messages.length > 0
        return (
          <div
            aria-hidden={!visible}
            className={`shrink-0 bg-white overflow-hidden transition-all duration-300 ease-out ${
              visible
                ? 'max-h-24 opacity-100 translate-y-0 border-t pt-1.5 pb-1'
                : 'max-h-0 opacity-0 translate-y-1 border-t-0 pt-0 pb-0'
            }`}
          >
            <div className="px-3">
              <div className="flex items-center gap-1.5 mb-1">
                <span className="text-[9px] font-semibold text-gray-400 uppercase tracking-wider">{t('chat.tryToUse')}</span>
                <span className="text-[9px] text-gray-300">
                  {t('chat.targetCounts', { hard: previewTargets.difficult.length, known: previewTargets.known.length })}
                </span>
              </div>
              <div className="flex flex-nowrap gap-1 overflow-x-auto" style={{ scrollbarWidth: 'none' }}>
                {targetChips.map((chip, i) => {
                  const expanded = detailIndex === i
                  const palette = chip.kind === 'hard'
                    ? { chip: 'bg-red-50 text-red-600 border-red-200', btn: 'hover:bg-red-100 text-red-500' }
                    : { chip: 'bg-blue-50 text-blue-600 border-blue-200', btn: 'hover:bg-blue-100 text-blue-500' }
                  return (
                    <span
                      key={`${chip.kind}-${chip.word}`}
                      className={`shrink-0 flex items-center gap-1 pl-2 pr-1 py-0.5 rounded-full text-xs font-medium border whitespace-nowrap ${palette.chip}`}
                    >
                      <span>{chip.word}</span>
                      <button
                        onClick={() => expanded ? closeDetail() : openDetail(i)}
                        aria-label={expanded ? t('chat.collapseWord') : t('chat.expandWord')}
                        className={`flex items-center justify-center w-4 h-4 rounded-full transition-colors ${palette.btn}`}
                      >
                        {expanded ? <UpOutlined style={{ fontSize: 8 }} /> : <DownOutlined style={{ fontSize: 8 }} />}
                      </button>
                    </span>
                  )
                })}
              </div>
            </div>
          </div>
        )
      })()}

      {/* Bottom input bar */}
      <div
        className="shrink-0 bg-white border-t px-3 py-2 safe-bottom mb-1"
        style={{ paddingBottom: `calc(env(safe-area-inset-bottom, 0px) + ${keyboardInset}px)` }}
      >
        {editingMessageId && (
          <div className="flex items-center gap-2 mb-1">
            <span className="text-[10px] font-semibold text-blue-500 uppercase tracking-wider">{t('common.edit')}</span>
            <button
              onClick={() => { setEditingMessageId(null); setInput('') }}
              className="text-[10px] text-gray-400 hover:text-gray-600 transition-colors"
            >
              {t('common.cancel')}
            </button>
          </div>
        )}
        {!editingMessageId && apiHost && apiKey && lang && lang !== explainLang && (
          <button
            onClick={() => setShowHelpReply(true)}
            disabled={loading}
            className="mb-1.5 flex items-center gap-1.5 px-3 py-1 rounded-full bg-indigo-50 text-indigo-700 text-xs font-medium disabled:opacity-50"
          >
            <BulbOutlined /> {t('chat.helpReply')}
          </button>
        )}
        <div className="flex items-end gap-2">
          <TextArea
            ref={inputRef}
            value={input}
            onChange={(e) => handleInputChange(e.target.value)}
            placeholder={editingMessageId ? t('chat.retypePlaceholder') : romajiInput ? t('chat.romajiPlaceholder') : t('chat.typeIn', { language: langName || t('chat.yourTarget') })}
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
            aria-label={t('chat.send')}
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

      {detailIndex != null && targetChips[detailIndex] && (
        <div
          className={`fixed inset-0 z-50 flex items-end sm:items-center justify-center bg-black/60 transition-opacity duration-200 ease-out ${
            detailVisible ? 'opacity-100' : 'opacity-0'
          }`}
          onClick={closeDetail}
          role="dialog"
          aria-modal="true"
        >
          <div
            className={`w-full sm:max-w-md bg-white rounded-t-2xl sm:rounded-2xl shadow-xl relative max-h-[85vh] flex flex-col transform transition-all duration-200 ease-out ${
              detailVisible
                ? 'opacity-100 translate-y-0 sm:scale-100'
                : 'opacity-0 translate-y-6 sm:translate-y-0 sm:scale-95'
            }`}
            onClick={e => e.stopPropagation()}
          >
            <div className="shrink-0 flex items-center justify-between px-4 pt-3 pb-2 border-b">
              <div className="text-xs text-gray-400">
                {detailIndex + 1} / {targetChips.length}
              </div>
              <button
                onClick={closeDetail}
                aria-label={t('common.close')}
                className="w-8 h-8 flex items-center justify-center rounded-full hover:bg-gray-100 transition-colors"
              >
                <CloseOutlined />
              </button>
            </div>

            <div className="relative flex-1 overflow-hidden">
              {detailIndex > 0 && (
                <button
                  onClick={() => openDetail(detailIndex - 1)}
                  aria-label={t('chat.previousWord')}
                  className="hidden sm:flex absolute left-1 top-1/2 -translate-y-1/2 z-10 w-8 h-8 items-center justify-center rounded-full bg-white/90 shadow border border-gray-200 hover:bg-gray-50"
                >
                  <LeftOutlined />
                </button>
              )}
              {detailIndex < targetChips.length - 1 && (
                <button
                  onClick={() => openDetail(detailIndex + 1)}
                  aria-label={t('chat.nextWord')}
                  className="hidden sm:flex absolute right-1 top-1/2 -translate-y-1/2 z-10 w-8 h-8 items-center justify-center rounded-full bg-white/90 shadow border border-gray-200 hover:bg-gray-50"
                >
                  <RightOutlined />
                </button>
              )}

              <div
                ref={detailScrollRef}
                onScroll={onDetailScroll}
                className="h-full overflow-x-auto overflow-y-auto flex snap-x snap-mandatory"
                style={{ scrollbarWidth: 'none' }}
              >
                {targetChips.map((chip, i) => {
                  const item = detailItems.get(chip.word)
                  const parsed = item?.detail ? parseDetailJson(item.detail) : null
                  return (
                    <div
                      key={`${chip.kind}-${chip.word}-${i}`}
                      ref={el => { if (el) detailCardRefs.current.set(i, el); else detailCardRefs.current.delete(i) }}
                      className="snap-center shrink-0 w-full px-5 py-4 overflow-y-auto"
                    >
                      <div className="flex items-center justify-center gap-2 mb-2">
                        <span className={`text-[10px] px-2 py-0.5 rounded-full text-white ${chip.kind === 'hard' ? 'bg-red-500' : 'bg-blue-500'}`}>
                          {chip.kind === 'hard' ? t('chat.difficult') : t('chat.known')}
                        </span>
                        {item?.state && (
                          <span className="text-[10px] px-2 py-0.5 rounded-full bg-gray-100 text-gray-600">
                            {t(`cardState.${item.state}`)}
                          </span>
                        )}
                      </div>
                      <div className="text-2xl font-semibold text-center mb-3">{chip.word}</div>

                      {!item ? (
                        <div className="text-center text-sm text-gray-400 py-6">
                          {t('chat.noSavedEntry')}
                        </div>
                      ) : (
                        <div className="space-y-2 text-left">
                          {parsed?.reading && (
                            <div className="flex items-baseline gap-2">
                              <span className="text-xs text-gray-400 w-24 shrink-0">{t('card.reading')}</span>
                              <span className="text-sm font-medium">{parsed.reading}</span>
                            </div>
                          )}
                          {parsed?.meaning && (
                            <div className="flex items-baseline gap-2">
                              <span className="text-xs text-gray-400 w-24 shrink-0">{t('card.meaning')}</span>
                              <span className="text-sm font-medium">{parsed.meaning}</span>
                            </div>
                          )}
                          {parsed?.part_of_speech && (
                            <div className="flex items-baseline gap-2">
                              <span className="text-xs text-gray-400 w-24 shrink-0">{t('card.partOfSpeech')}</span>
                              <span className="text-xs px-2 py-0.5 rounded-full bg-gray-100 text-gray-600">{partOfSpeechLabel(parsed.part_of_speech)}</span>
                            </div>
                          )}
                          {parsed?.example && (
                            <div className="mt-2 p-2 bg-gray-50 rounded-lg">
                              <p className="text-sm italic">{parsed.example}</p>
                              {parsed.example_meaning && (
                                <p className="text-xs text-gray-500 mt-1">{parsed.example_meaning}</p>
                              )}
                            </div>
                          )}
                          {!parsed && item.detail && (
                            <p className="text-sm text-gray-600 whitespace-pre-wrap">{item.detail}</p>
                          )}
                          {!item.detail && (
                            <p className="text-sm text-gray-400">{t('quiz.noMeaningYet', { language: languageName(explainLang) })}</p>
                          )}
                          {item.mnemonic && (
                            <div className="pt-2 mt-2 border-t border-gray-100">
                              <span className="text-xs text-gray-400">{t('card.mnemonic')}</span>
                              <p className="text-sm text-amber-700 bg-amber-50 p-2 rounded-lg mt-1">{item.mnemonic}</p>
                            </div>
                          )}
                        </div>
                      )}
                    </div>
                  )
                })}
              </div>
            </div>

            {targetChips.length > 1 && (
              <div className="shrink-0 flex items-center justify-center gap-1 py-2 border-t">
                {targetChips.map((_, i) => (
                  <button
                    key={i}
                    onClick={() => openDetail(i)}
                    aria-label={t('chat.goToWord', { n: i + 1 })}
                    className={`h-1.5 rounded-full transition-all ${i === detailIndex ? 'w-4 bg-gray-600' : 'w-1.5 bg-gray-300'}`}
                  />
                ))}
              </div>
            )}
          </div>
        </div>
      )}

      {wordSheet && (
        <WordSheet
          word={wordSheet.word}
          base={wordSheet.base}
          language={lang || 'ja'}
          api={{ apiHost, apiKey, model: localStorage.getItem('api_vocab_model') || model }}
          onAdd={addWordFromSheet}
          onClose={() => setWordSheet(null)}
        />
      )}

      {showHelpReply && (
        <HelpReplySheet
          language={lang || 'ja'}
          levelRule={lang === 'ja' ? JLPT_LEVELS[jlptLevel]?.promptRule ?? null : null}
          context={visibleMessages.filter(m => !m.content.startsWith('[Error]')).map(m => ({ role: m.role, content: m.content }))}
          api={{ apiHost, apiKey, model }}
          onUse={(text) => { setInput(text); setShowHelpReply(false); setTimeout(() => inputRef.current?.focus(), 0) }}
          onClose={() => setShowHelpReply(false)}
        />
      )}
    </div>
  )
}
