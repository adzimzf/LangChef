import { useEffect, useMemo, useRef, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { splitTappable, isWordSegment } from '../lib/unknown'
import { renderFurigana, hasFuriganaAnnotations, stripFurigana } from '../lib/furigana'
import { difficultyColor } from '../lib/srs'
import { JLPT_LEVELS, DEFAULT_JLPT_LEVEL, JLPT_LEVEL_STORAGE_KEY } from '../lib/jlpt'
import type { JLPTLevel } from '../lib/jlpt'
import { Input } from 'antd'
const { TextArea } = Input
import { ArrowLeftOutlined, SendOutlined } from '@ant-design/icons'
import { useNavigate, useSearchParams } from 'react-router-dom'

type Msg = { role: 'user' | 'assistant' | 'system', content: string }

const LANGUAGES: Record<string, { label: string, flag: string }> = {
  ja: { label: 'Japanese', flag: '🇯🇵' },
  id: { label: 'Indonesian', flag: '🇮🇩' },
}

const SUGGESTIONS = ['Travel', 'Food', 'Work', 'Daily routine', 'Hobbies']

type GrammarDBItem = { id: string; pattern: string; detail: string; language: string; level: string; is_default: number }

type ChipItem = { type: 'topic' | 'grammar'; label: string; action: string }

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
  const [difficult, setDifficult] = useState<Map<string, { EF: number; R: number }>>(new Map())
  const [known, setKnown] = useState<Set<string>>(new Set())
  const [loading, setLoading] = useState(false)
  const [showFurigana, setShowFurigana] = useState(false)
  const [showMoreChips, setShowMoreChips] = useState(false)
  const [grammarItems, setGrammarItems] = useState<GrammarDBItem[]>([])
  const logRef = useRef<HTMLDivElement | null>(null)
  const inputRef = useRef<any>(null)
  const chipsScrollRef = useRef<HTMLDivElement | null>(null)
  const [hasChipOverflow, setHasChipOverflow] = useState(false)

  const langInfo = LANGUAGES[lang]

  // Build chip items: topics + grammar (loaded from DB)
  const allChips = useMemo<ChipItem[]>(() => {
    const items: ChipItem[] = SUGGESTIONS.map(s => ({
      type: 'topic',
      label: s,
      action: `Talk about ${s.toLowerCase()}.`,
    }))
    if (lang === 'ja') {
      grammarItems.forEach(g => {
        items.push({
          type: 'grammar',
          label: g.pattern,
          action: `Let's practice the grammar pattern: ${g.pattern}. ${g.detail ? `(${g.detail}) ` : ''}Please use it in a sentence and explain it.`,
        })
      })
    }
    return items
  }, [lang, grammarItems])

  // Detect chip overflow — compare scrollWidth vs clientWidth on the scroll container
  useEffect(() => {
    function checkOverflow() {
      const el = chipsScrollRef.current
      if (!el) return
      setHasChipOverflow(el.scrollWidth > el.clientWidth + 2)
    }
    // Delay to ensure layout is painted
    const id = requestAnimationFrame(() => {
      requestAnimationFrame(checkOverflow)
    })
    window.addEventListener('resize', checkOverflow)
    return () => {
      cancelAnimationFrame(id)
      window.removeEventListener('resize', checkOverflow)
    }
  }, [allChips, hasChipOverflow])

  // Close dropdown on outside click
  useEffect(() => {
    if (!showMoreChips) return
    function handleClick(e: MouseEvent) {
      const target = e.target as HTMLElement
      if (target.closest('[data-chips-dropdown]') || target.closest('[data-more-btn]')) return
      setShowMoreChips(false)
    }
    document.addEventListener('click', handleClick)
    return () => document.removeEventListener('click', handleClick)
  }, [showMoreChips])

  // Load settings
  useEffect(() => {
    const host = localStorage.getItem('api_host') || ''
    const key = localStorage.getItem('api_key') || ''
    const m = localStorage.getItem('api_model') || 'gpt-3.5-turbo'
    setApiHost(host)
    setApiKey(key)
    setModel(m)
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
        const res = await invoke<{ role: string, content: string, created_at: number }[]>('db_list_session_messages', { sessionId })
        setMessages(res.map(m => ({ role: m.role as Msg['role'], content: m.content })))
      } catch {}
    }
    load()
  }, [sessionId])

  // Load difficult words from review_items (with EF and R for color)
  useEffect(() => {
    async function load() {
      try {
        const items = await invoke<{ id: string, content: string, EF: number, I: number, R: number, tags: string | null }[]>('db_get_review_items')
        const hardMap = new Map<string, { EF: number; R: number }>()
        items.forEach(i => {
          // Words tagged 'hard' OR new/learning words (R < 3) need reinforcement
          if (i.tags === 'hard' || i.R < 3) hardMap.set(i.content.toLowerCase(), { EF: i.EF, R: i.R })
        })
        setDifficult(hardMap)
        const knownWords = new Set(items.filter(i => i.R >= 3).map(i => i.content.toLowerCase()))
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

  // Auto-scroll
  useEffect(() => {
    if (logRef.current) {
      logRef.current.scrollTop = logRef.current.scrollHeight
    }
  }, [messages.length, loading])

  async function markWordDifficult(word: string) {
    const w = word.toLowerCase()
    const next = new Map(difficult)
    next.set(w, { EF: 2.5, R: 0 })
    setDifficult(next)
    try {
      await invoke('db_upsert_review_item', { content: w, tag: 'hard', language: lang || null })
    } catch (e) {
      console.error('Failed to save difficult word to DB:', e)
      // Roll back local state if DB write fails
      const reverted = new Map(difficult)
      reverted.delete(w)
      setDifficult(reverted)
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

  function buildSSRSystemPrompt(): string {
    const difficultArr = Array.from(difficult.keys())
    const knownArr = Array.from(known).filter(w => !difficult.has(w))

    // Split: 50% difficult words, 50% known/remembered words
    const halfDiff = Math.ceil(difficultArr.length / 2)
    const halfKnown = Math.ceil(knownArr.length / 2)
    const selectedDifficult = difficultArr.slice(0, halfDiff)
    const selectedKnown = knownArr.slice(0, halfKnown)

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
    return prompt
  }

  async function send() {
    if (!input.trim()) return
    if (!sessionId) {
      try {
        const id = await invoke<string>('db_new_session', { title: null, language: lang || null })
        setSessionId(id)
      } catch {}
    }
    const user: Msg = { role: 'user', content: input }
    const nextMessages = [...messages, user]
    setMessages(nextMessages)
    await invoke('db_add_message_in_session', { sessionId, role: 'user', content: user.content }).catch(() => {})
    setInput('')
    await invoke('db_increment_session_today').catch(() => {})
    setTimeout(() => {
      if (logRef.current) logRef.current.scrollTop = logRef.current.scrollHeight
    }, 0)

    if (apiHost && apiKey) {
      setLoading(true)
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
        const assistant: Msg = { role: 'assistant', content: res }
        setMessages(prev => [...prev, assistant])
        await invoke('db_add_message_in_session', { sessionId, role: 'assistant', content: res }).catch(() => {})
      } catch (e) {
        const errMsg = e instanceof Error ? e.message : String(e)
        setMessages(prev => [...prev, { role: 'assistant', content: `[Error] ${errMsg}` }])
        await invoke('db_add_message_in_session', { sessionId, role: 'assistant', content: `[Error] ${errMsg}` }).catch(() => {})
      } finally {
        setLoading(false)
      }
    } else {
      const fallback = `Let's talk about it. Which parts feel tricky for you? Try rephrasing in the target language.`
      setMessages(prev => [...prev, { role: 'assistant', content: fallback }])
      await invoke('db_add_message_in_session', { sessionId, role: 'assistant', content: fallback }).catch(() => {})
    }
  }

  function renderAssistantContent(content: string) {
    if (content.startsWith('[Error]')) {
      return <span className="text-red-500 text-sm">{content}</span>
    }
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
        const diffInfo = difficult.get(w)
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
      const diffInfo = difficult.get(w)
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
    <div className="flex flex-col h-dvh bg-gray-50">
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

      {/* Suggestion chips with overflow */}
      <div className="shrink-0 bg-white border-b px-3 py-2 relative flex items-center gap-2">
        <div
          ref={chipsScrollRef}
          className="flex gap-2 overflow-hidden flex-1 min-w-0"
        >
          {allChips.map(chip => (
            <button
              key={chip.label}
              onClick={() => setInput(chip.action)}
              className={`shrink-0 whitespace-nowrap px-3 py-1.5 rounded-full text-sm transition-colors ${
                chip.type === 'grammar'
                  ? 'bg-indigo-50 text-indigo-600 hover:bg-indigo-100 active:bg-indigo-200'
                  : 'bg-gray-100 text-gray-700 hover:bg-blue-50 hover:text-blue-600 active:bg-blue-100'
              }`}
            >
              {chip.label}
            </button>
          ))}
        </div>
        {hasChipOverflow && (
          <button
            data-more-btn
            onClick={() => setShowMoreChips(!showMoreChips)}
            className="shrink-0 px-3 py-1.5 rounded-full text-sm bg-gray-200 text-gray-600 hover:bg-gray-300 active:bg-gray-400 transition-colors flex items-center gap-1"
          >
            More
            <svg className={`w-3 h-3 transition-transform ${showMoreChips ? 'rotate-180' : ''}`} fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 9l-7 7-7-7" />
            </svg>
          </button>
        )}

        {/* Floating scrollable dropdown for all chips */}
        {showMoreChips && hasChipOverflow && (
          <div
            data-chips-dropdown
            className="absolute right-3 top-full mt-1 bg-white rounded-xl shadow-lg border border-gray-200 z-20 py-2 w-56 max-h-60 overflow-y-auto"
          >
            <div className="px-2 space-y-1">
              {allChips.map(chip => (
                <button
                  key={chip.label}
                  onClick={() => {
                    setInput(chip.action)
                    setShowMoreChips(false)
                  }}
                  className={`w-full text-left px-3 py-2 rounded-lg text-sm transition-colors ${
                    chip.type === 'grammar'
                      ? 'bg-indigo-50 text-indigo-600 hover:bg-indigo-100 active:bg-indigo-200'
                      : 'text-gray-700 hover:bg-gray-100 active:bg-gray-200'
                  }`}
                >
                  {chip.label}
                </button>
              ))}
            </div>
          </div>
        )}
      </div>

      {/* Messages area */}
      <div ref={logRef} className="flex-1 overflow-y-auto px-3 py-3">
        {messages.length === 0 && !loading && (
          <div className="flex flex-col items-center justify-center h-full text-gray-400 gap-3">
            <div className="text-4xl">💬</div>
            <p className="text-sm text-center">Start a conversation in {langInfo?.label || 'your target language'}</p>
            <p className="text-xs text-center text-blue-400">Tap words in replies to mark them difficult</p>
          </div>
        )}
        {messages.map((m, i) => (
          <div
            key={i}
            className={`flex mb-2 ${m.role === 'user' ? 'justify-end' : 'justify-start'}`}
          >
            <div
              className={`
                max-w-[80%] px-3 py-2 text-[15px] leading-relaxed
                ${m.role === 'user'
                  ? 'bg-blue-500 text-white rounded-2xl rounded-br-md'
                  : 'bg-white text-gray-800 rounded-2xl rounded-bl-md shadow-sm'
                }
              `}
            >
              {m.role === 'assistant' ? renderAssistantContent(m.content) : m.content}
            </div>
          </div>
        ))}
        {loading && <TypingIndicator />}
      </div>

      {/* Bottom input bar */}
      <div className="shrink-0 bg-white border-t px-3 py-2 safe-bottom mb-1">
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
            onClick={send}
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
