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
import { ArrowLeftOutlined, SendOutlined, InfoCircleOutlined, CloseOutlined, PlusOutlined } from '@ant-design/icons'
import { useNavigate, useSearchParams } from 'react-router-dom'
import ReactMarkdown from 'react-markdown'

type Msg = { id: number | null, role: 'user' | 'assistant' | 'system', content: string }

const LANGUAGES: Record<string, { label: string, flag: string }> = {
  ja: { label: 'Japanese', flag: '🇯🇵' },
  id: { label: 'Indonesian', flag: '🇮🇩' },
}

const SUGGESTIONS = ['Travel', 'Food', 'Work', 'Daily routine', 'Hobbies']

const MAX_REVIEWS_DEFAULT = 100
const MAX_REVIEWS_KEY = 'max_reviews_per_day'
const MAX_NEW_CARDS_DEFAULT = 20
const MAX_NEW_CARDS_KEY = 'max_new_cards_per_day'
const MAX_DIFFICULT_WORDS_KEY = 'max_difficult_words'
const MAX_DIFFICULT_WORDS_DEFAULT = 10

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
  const [colorDifficult, setColorDifficult] = useState<Map<string, { EF: number; R: number; isNew: boolean }>>(new Map())
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
  const logRef = useRef<HTMLDivElement | null>(null)
  const inputRef = useRef<any>(null)
  // FIFO queue index for rotating through difficult/known words per message
  const wordQueueIndex = useRef<{ difficult: number; known: number }>({ difficult: 0, known: 0 })
  // Track whether component is mounted — prevents stale state updates after navigation
  const mounted = useRef(true)
  const [keyboardInset, setKeyboardInset] = useState(0)

  const langInfo = LANGUAGES[lang]

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
        const res = await invoke<{ id: number, role: string, content: string, created_at: number }[]>('db_list_session_messages', { sessionId })
        setMessages(res.map(m => ({ id: m.id, role: m.role as Msg['role'], content: m.content })))
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
          setMessages(res.map(m => ({ id: m.id, role: m.role as Msg['role'], content: m.content })))
        }
      } catch {}
    }, 1500)
    return () => clearInterval(interval)
  }, [sessionId])

  // Load difficult words from review_items
  // Two separate queries:
  //   - LLM prompt words: all items, classified by easeScore (same as Word List default)
  //   - Color rendering: due by next week (broader visual feedback)
  useEffect(() => {
    async function load() {
      try {
        // ── Query 1: For LLM system prompt — all items, sorted hardest-first (same as Word List) ──
        const allItems = await invoke<ReviewItemRaw[]>('db_get_review_items')

        // Sort by ease score (hardest first) — same as WordList default sortOrder
        allItems.sort((a, b) => easeScore(a.EF, a.R) - easeScore(b.EF, b.R))

        const hardMap = new Map<string, { EF: number; R: number; isNew: boolean }>()
        let newDifficultCount = 0

        // Classify using easeScore — hard (< 0.3) or medium (0.3–0.7) are difficult
        allItems.forEach(i => {
          const ease = easeScore(i.EF, i.R)
          if (ease < 0.7) {
            const isNew = i.state === 'new' || !i.state
            hardMap.set(i.content.toLowerCase(), { EF: i.EF, R: i.R, isNew })
            if (isNew) newDifficultCount++
          }
        })
        setDifficult(hardMap)

        // Known words: easy items (ease >= 0.7) — same metric as WordList
        const knownWords = new Set<string>()
        allItems.filter(i => easeScore(i.EF, i.R) >= 0.7).forEach(i => {
          knownWords.add(i.content.toLowerCase())
        })
        setKnown(knownWords)

        // ── Query 2: For color rendering — due by next week, broader window ──
        const dNextWeek = new Date()
        dNextWeek.setDate(dNextWeek.getDate() + 7)
        dNextWeek.setHours(23, 59, 59, 999)

        const colorItems = await invoke<ReviewItemRaw[]>('db_get_due_review_items', {
          language: null,
          limit: 500,
          newCardsRemaining: 50,
          maxNextReview: Math.floor(dNextWeek.getTime() / 1000),
        })

        const colorMap = new Map<string, { EF: number; R: number; isNew: boolean }>()
        colorItems.forEach(i => {
          if ((i.tags === 'hard' || i.tags === 'medium') || i.R < 3) {
            colorMap.set(i.content.toLowerCase(), { EF: i.EF, R: i.R, isNew: i.state === 'new' || !i.state })
          }
        })
        setColorDifficult(colorMap)
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
    // Also add to color map so it renders immediately
    const colorNext = new Map(colorDifficult)
    colorNext.set(w, { EF: 2.5, R: 0, isNew: true })
    setColorDifficult(colorNext)
    try {
      await invoke('db_upsert_review_item', { content: w, language: lang || null })
    } catch (e) {
      console.error('Failed to save difficult word to DB:', e)
      // Roll back local state if DB write fails
      const reverted = new Map(difficult)
      reverted.delete(w)
      setDifficult(reverted)
      const colorReverted = new Map(colorDifficult)
      colorReverted.delete(w)
      setColorDifficult(colorReverted)
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

  function buildSSRSystemPrompt(): string {
    const difficultArr = Array.from(difficult.keys())
    const knownArr = Array.from(known).filter(w => !difficult.has(w))

    // Use configured max difficult words count
    const maxWords = getMaxDifficultWords()
    // Rotate through words via FIFO queue instead of always taking first N
    const [selectedDifficult, nextDifficultIdx] = dequeueRotate(difficultArr, wordQueueIndex.current.difficult, maxWords)
    const [selectedKnown, nextKnownIdx] = dequeueRotate(knownArr, wordQueueIndex.current.known, maxWords)
    // Advance queue positions for next message
    wordQueueIndex.current = { difficult: nextDifficultIdx, known: nextKnownIdx }

    const langName = langInfo?.label || 'the target language'

    let prompt = `You are a language learning conversation partner. The target language is ${langName}. Reply in ${langName} only. `
    prompt += `Keep the conversation natural and engaging. `

    if (lang === 'ja') {
      prompt += `\n\nFURIGANA RULE: For every word containing kanji in your response, you MUST annotate it with its reading using this format: {kanji|reading}. Examples: {日本|にほん}, {食べ|た}べ{物|もの}, {先生|せんせい}, {行|い}きます. Annotate EACH kanji or kanji compound with its correct reading in context. Do NOT annotate hiragana, katakana, or non-kanji text. Place the annotation around the kanji portion only. Example: "{食|た}べた{後|あと}で{散歩|さんぽ}しました"`

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

    if (selectedDifficult.length > 0 || selectedKnown.length > 0) {
      prompt += `\n\nCRITICAL SSR RULE — YOU MUST FOLLOW THIS:\n`
      if (selectedDifficult.length > 0) {
        prompt += `- DIFFICULT words (you MUST use ALL of these in your response): ${selectedDifficult.join(', ')}\n`
        prompt += `  These are words the learner is struggling with. Using them reinforces memory. Do NOT skip any.\n`
      }
      if (selectedKnown.length > 0) {
        prompt += `- KNOWN words (use at least half of these to maintain retention): ${selectedKnown.join(', ')}\n`
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

  async function send(message?: string) {
    const msg = message?.trim() || input.trim()
    if (!msg) return
    if (!sessionId) {
      try {
        const id = await invoke<string>('db_new_session', { title: null, language: lang || null, level: lang === 'ja' ? jlptLevel : null })
        setSessionId(id)
      } catch {}
    }
    const user: Msg = { id: null, role: 'user', content: msg }
    const nextMessages = [...messages, user]
    setMessages(nextMessages)
    const userId = await invoke<number>('db_add_message_in_session', { sessionId, role: 'user', content: user.content }).catch(() => -1)
    if (userId >= 0) {
      setMessages(prev => prev.map((m, idx) => idx === nextMessages.length - 1 ? { ...m, id: userId } : m))
    }
    // Spawn feedback evaluation in parallel (non-blocking, runs alongside main LLM chat)
    // Skip auto-sent topic suggestions (e.g., "Talk about food.")
    if (userId >= 0 && apiHost && apiKey && !/^talk about /i.test(msg)) {
      setFeedbackMap(prev => new Map(prev).set(userId, { messageId: userId, score: 0, feedback: '', loading: true }))
      // Build context from previous messages (exclude the current one being evaluated)
      const contextMsgs = nextMessages.slice(0, -1).map(m => ({ role: m.role as string, content: m.content }))
      invoke<UserFeedback>('db_evaluate_user_message', {
        apiHost, apiKey, model,
        messageId: userId,
        messageContent: msg,
        language: lang || '',
        level: lang === 'ja' ? jlptLevel : '',
        contextMessages: contextMsgs,
      }).then(fb => {
        setFeedbackMap(prev => { const next = new Map(prev); next.set(userId, { ...fb, messageId: userId, loading: false }); return next })
      }).catch(() => {
        setFeedbackMap(prev => { const next = new Map(prev); const existing = next.get(userId); if (existing?.loading) next.delete(userId); return next })
      })
    }
    setInput('')
    await invoke('db_increment_session_today').catch(() => {})
    setTimeout(() => {
      if (logRef.current) logRef.current.scrollTop = logRef.current.scrollHeight
    }, 0)

    if (apiHost && apiKey) {
      setLoading(true)
      // Mark LLM as pending so typing indicator persists across navigation
      if (sessionId) setLLMPending(sessionId)
      try {
        // Build SSR system prompt
        const systemPrompt = buildSSRSystemPrompt()

        // Send full conversation history with system prompt
        const llmMessages = [
          { role: 'system' as const, content: systemPrompt },
          ...nextMessages.map(m => ({ role: m.role as string, content: m.content })),
        ]

        const res = await invoke<string>('llm_chat', {
          apiHost,
          apiKey,
          model,
          messages: llmMessages,
        })

        // Always save assistant response to DB so it persists across navigation
        const assistantId = await invoke<number>('db_add_message_in_session', { sessionId, role: 'assistant', content: res }).catch(() => -1)

        // Only update React state if component is still mounted
        if (mounted.current) {
          const assistant: Msg = { id: null, role: 'assistant', content: res }
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
        const errorId = await invoke<number>('db_add_message_in_session', { sessionId, role: 'assistant', content: errorContent }).catch(() => -1)

        // Only update React state if component is still mounted
        if (mounted.current) {
          const errorMsg: Msg = { id: null, role: 'assistant', content: errorContent }
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
      const fallbackMsg: Msg = { id: null, role: 'assistant', content: fallback }
      const fallbackIdx = nextMessages.length
      setMessages(prev => [...prev, fallbackMsg])
      const fallbackId = await invoke<number>('db_add_message_in_session', { sessionId, role: 'assistant', content: fallback }).catch(() => -1)
      if (fallbackId >= 0) {
        setMessages(prev => prev.map((m, idx) => idx === fallbackIdx ? { ...m, id: fallbackId } : m))
      }
    }
  }

  function renderAssistantContent(content: string) {
    if (content.startsWith('[Error]')) {
      return <span className="text-red-500 text-sm">{content}</span>
    }
    return renderTextSegment(content)
  }

  function renderTextSegment(content: string) {
    const locale = lang === 'id' ? 'id' : 'ja'
    const isJapanese = lang === 'ja'

    // For Japanese, the LLM returns {kanji|reading} annotations.
    // We strip annotations to get plain text for word segmentation,
    // then render each segment with <ruby> tags from the annotated content.
    if (isJapanese && hasFuriganaAnnotations(content)) {
      const plain = stripFurigana(content)
      const parts = splitTappable(plain, 'ja')

      // Walk through the annotated content alongside the plain text segments
      // to produce per-segment annotated slices
      let annotIdx = 0
      const annotated = content

      return parts.map((part, idx) => {
        if (!isWordSegment(part)) {
          // Non-word segment: advance through annotated text by matching
          // plain characters (skipping {reading|} parts)
          const slice = sliceAnnotated(annotated, annotIdx, part.length)
          annotIdx += part.length // advance by plain-text length
          return <span key={idx}>{renderFurigana(slice, showFurigana)}</span>
        }

        const w = part.toLowerCase()
        const diffInfo = colorDifficult.get(w)
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
      const diffInfo = colorDifficult.get(w)
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
            <div className="flex flex-wrap justify-center gap-2 mt-1">
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
        )}
        {messages.map((m, i) => (
          <div key={i} className={`flex mb-2 ${m.role === 'user' ? 'justify-end flex-col items-end' : 'justify-start'}`}>
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
                  {renderAssistantContent(m.content)}
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

            {/* Feedback badge for user messages */}
            {m.role === 'user' && <UserMessageFeedback feedback={m.id ? feedbackMap.get(m.id) : undefined} />}
          </div>
        ))}
        {loading && <TypingIndicator />}
      </div>

      {/* Bottom input bar */}
      <div
        className="shrink-0 bg-white border-t px-3 py-2 safe-bottom mb-1"
        style={{ paddingBottom: `calc(env(safe-area-inset-bottom, 0px) + ${keyboardInset}px)` }}
      >
        <div className="flex items-end gap-2">
          <TextArea
            ref={inputRef}
            value={input}
            onChange={(e) => setInput(e.target.value)}
            placeholder={`Type in ${langInfo?.label || 'target language'}...`}
            className="flex-1 rounded-2xl bg-gray-100 border-none"
            style={{ borderRadius: 20, padding: '8px 16px', resize: 'none' }}
            autoSize={{ minRows: 1, maxRows: 4 }}
            disabled={loading}
          />
          <button
            onClick={() => send()}
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
