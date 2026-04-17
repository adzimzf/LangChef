import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { PlusOutlined, CloseOutlined, EditOutlined, DeleteOutlined } from '@ant-design/icons'
import { useNavigate } from 'react-router-dom'
import { JLPT_LEVELS, JLPT_LEVEL_ORDER, DEFAULT_JLPT_LEVEL, JLPT_LEVEL_STORAGE_KEY } from '../lib/jlpt'
import type { JLPTLevel } from '../lib/jlpt'

type GrammarLevelInfo = { level: string; count: number }

type ChatSession = {
  id: string
  title?: string | null
  language?: string | null
  created_at: number
  last_activity: number
}

const LANGUAGES = [
  { code: 'ja', label: 'Japanese', flag: '🇯🇵' },
  { code: 'id', label: 'Indonesian', flag: '🇮🇩' },
]

const AVATAR_COLORS = [
  'bg-blue-500', 'bg-green-500', 'bg-purple-500',
  'bg-orange-500', 'bg-pink-500', 'bg-teal-500',
]

function getInitial(title: string | null | undefined): string {
  if (!title) return 'C'
  return title.charAt(0).toUpperCase()
}

function getColor(id: string): string {
  let hash = 0
  for (let i = 0; i < id.length; i++) hash = id.charCodeAt(i) + ((hash << 5) - hash)
  return AVATAR_COLORS[Math.abs(hash) % AVATAR_COLORS.length]
}

function timeAgo(ts: number): string {
  const now = Date.now()
  const target = ts * 1000
  const diffMs = now - target
  if (diffMs < 0) return 'Just now'
  const absDiff = diffMs

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

  return `${value}${unit} ago`
}

function getLangLabel(code: string | null | undefined): string {
  if (!code) return ''
  const lang = LANGUAGES.find(l => l.code === code)
  return lang ? `${lang.flag} ${lang.label}` : code
}

export default function Chats() {
  const [sessions, setSessions] = useState<ChatSession[]>([])
  const [showLangPicker, setShowLangPicker] = useState(false)
  const [showJlptPicker, setShowJlptPicker] = useState(false)
  const [selectedLang, setSelectedLang] = useState('')
  const [jlptLevel, setJlptLevel] = useState<JLPTLevel>(
    (localStorage.getItem(JLPT_LEVEL_STORAGE_KEY) as JLPTLevel) || DEFAULT_JLPT_LEVEL
  )
  const [editingId, setEditingId] = useState<string | null>(null)
  const [editTitle, setEditTitle] = useState('')
  const [deletingId, setDeletingId] = useState<string | null>(null)
  const [dbLevels, setDbLevels] = useState<string[]>(JLPT_LEVEL_ORDER)
  const navigate = useNavigate()

  async function load() {
    try {
      const res = await invoke<ChatSession[]>('db_list_sessions')
      setSessions(res)
    } catch {}
  }

  // Load grammar levels from DB
  useEffect(() => {
    async function loadLevels() {
      try {
        const levels = await invoke<string[]>('db_get_grammar_levels', { language: 'ja' })
        if (levels.length > 0) setDbLevels(levels)
      } catch {}
    }
    loadLevels()
  }, [])

  useEffect(() => {
    load()
  }, [])

  async function newChat(language: string) {
    if (language === 'ja') {
      // For Japanese, show JLPT level picker first
      setSelectedLang(language)
      setShowLangPicker(false)
      // Delay to avoid the backdrop click from the language picker
      // immediately closing the JLPT picker that replaces it
      setTimeout(() => setShowJlptPicker(true), 50)
      return
    }
    try {
      const id = await invoke<string>('db_new_session', { title: null, language })
      setShowLangPicker(false)
      navigate(`/chat?sid=${id}&lang=${language}`)
    } catch {}
  }

  async function startJlptChat(level: JLPTLevel) {
    // Persist the chosen level for next time
    localStorage.setItem(JLPT_LEVEL_STORAGE_KEY, level)
    setJlptLevel(level)
    try {
      const id = await invoke<string>('db_new_session', { title: null, language: selectedLang })
      setShowJlptPicker(false)
      navigate(`/chat?sid=${id}&lang=${selectedLang}&jlpt=${level}`)
    } catch {}
  }

  async function renameSession(id: string, title: string) {
    try {
      await invoke('db_rename_session', { sessionId: id, title })
      setSessions(prev => prev.map(s => s.id === id ? { ...s, title } : s))
    } catch {}
    setEditingId(null)
  }

  async function deleteSession(id: string) {
    try {
      await invoke('db_delete_session', { sessionId: id })
      setSessions(prev => prev.filter(s => s.id !== id))
    } catch {}
    setDeletingId(null)
  }

  function startEdit(s: ChatSession) {
    setEditingId(s.id)
    setEditTitle(s.title || '')
  }

  return (
    <div className="flex flex-col h-full overflow-y-auto overflow-x-hidden">
      <div className="sticky top-0 z-10 py-3 bg-gray-50 flex items-center justify-between shrink-0">
        <h2 className="text-lg font-semibold">Chats</h2>
        <button
          onClick={() => setShowLangPicker(true)}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-full bg-blue-500 text-white text-sm font-medium hover:bg-blue-600 active:bg-blue-700 transition-colors"
        >
          <PlusOutlined />
          <span>New</span>
        </button>
      </div>

      {/* Language picker modal */}
      {showLangPicker && (
        <div className="fixed inset-0 z-50 flex items-end justify-center bg-black/30" onClick={() => setShowLangPicker(false)}>
          <div
            className="w-full max-w-lg bg-white rounded-t-2xl p-5 pb-[72px] safe-bottom"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="flex items-center justify-between mb-4">
              <h3 className="text-base font-semibold">Choose Language</h3>
              <button
                onClick={() => setShowLangPicker(false)}
                className="w-8 h-8 flex items-center justify-center rounded-full hover:bg-gray-100 active:bg-gray-200"
              >
                <CloseOutlined />
              </button>
            </div>
            <div className="space-y-2">
              {LANGUAGES.map(lang => (
                <button
                  key={lang.code}
                  onClick={() => newChat(lang.code)}
                  className="w-full flex items-center gap-3 px-4 py-3 rounded-xl hover:bg-gray-50 active:bg-gray-100 transition-colors text-left"
                >
                  <span className="text-2xl">{lang.flag}</span>
                  <span className="text-[15px] font-medium">{lang.label}</span>
                </button>
              ))}
            </div>
          </div>
        </div>
      )}

      {/* JLPT level picker modal */}
      {showJlptPicker && (
        <div className="fixed inset-0 z-50 flex items-end justify-center bg-black/30" onClick={() => setShowJlptPicker(false)}>
          <div
            className="w-full max-w-lg bg-white rounded-t-2xl p-5 pb-[72px] safe-bottom"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="flex items-center justify-between mb-4">
              <h3 className="text-base font-semibold">Choose Level</h3>
              <button
                onClick={() => setShowJlptPicker(false)}
                className="w-8 h-8 flex items-center justify-center rounded-full hover:bg-gray-100 active:bg-gray-200"
              >
                <CloseOutlined />
              </button>
            </div>
            <p className="text-xs text-gray-500 mb-3">This sets the grammar difficulty and sentence length for the conversation.</p>
            <div className="space-y-2">
              {dbLevels.map(level => {
                const jlptInfo = JLPT_LEVELS[level as JLPTLevel]
                return (
                  <button
                    key={level}
                    onClick={() => startJlptChat(level as JLPTLevel)}
                    className={`w-full flex items-center gap-3 px-4 py-3 rounded-xl transition-colors text-left ${
                      jlptLevel === level
                        ? 'bg-blue-50 ring-1 ring-blue-200'
                        : 'hover:bg-gray-50 active:bg-gray-100'
                    }`}
                  >
                    <span className="text-[15px] font-semibold w-10 text-center">{level}</span>
                    <span className="text-sm text-gray-600">{jlptInfo?.description || `Grammar level ${level}`}</span>
                  </button>
                )
              })}
            </div>
          </div>
        </div>
      )}

      {/* Delete confirmation modal */}
      {deletingId && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/30" onClick={() => setDeletingId(null)}>
          <div
            className="bg-white rounded-2xl p-5 mx-6 max-w-sm w-full"
            onClick={(e) => e.stopPropagation()}
          >
            <h3 className="text-base font-semibold mb-2">Delete Chat</h3>
            <p className="text-sm text-gray-500 mb-4">This will permanently delete this conversation and all its messages. This cannot be undone.</p>
            <div className="flex gap-2 justify-end">
              <button
                onClick={() => setDeletingId(null)}
                className="px-4 py-2 rounded-xl text-sm font-medium hover:bg-gray-100 active:bg-gray-200 transition-colors"
              >
                Cancel
              </button>
              <button
                onClick={() => deleteSession(deletingId)}
                className="px-4 py-2 rounded-xl text-sm font-medium bg-red-500 text-white hover:bg-red-600 active:bg-red-700 transition-colors"
              >
                Delete
              </button>
            </div>
          </div>
        </div>
      )}

      {sessions.length === 0 ? (
        <div className="flex flex-col items-center justify-center flex-1 text-gray-400 gap-3 py-12">
          <div className="text-4xl">📝</div>
          <p className="text-sm">No conversations yet</p>
          <button
            onClick={() => setShowLangPicker(true)}
            className="px-4 py-2 rounded-full bg-blue-500 text-white text-sm font-medium hover:bg-blue-600 active:bg-blue-700 transition-colors"
          >
            Start a chat
          </button>
        </div>
      ) : (
        <div className="flex flex-col -mx-4 overflow-x-hidden">
          {sessions.map(s => (
            <div
              key={s.id}
              className="flex items-center gap-3 px-4 py-3 hover:bg-gray-50 active:bg-gray-100 transition-colors border-b border-gray-50"
            >
              <div
                className={`w-11 h-11 rounded-full ${getColor(s.id)} flex items-center justify-center text-white font-semibold text-base shrink-0 cursor-pointer`}
                onClick={() => navigate(`/chat?sid=${s.id}&lang=${s.language || ''}`)}
              >
                {getInitial(s.title)}
              </div>

              {editingId === s.id ? (
                <div className="flex-1 min-w-0 flex items-center gap-2">
                  <input
                    autoFocus
                    value={editTitle}
                    onChange={(e) => setEditTitle(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter') {
                        e.preventDefault()
                        renameSession(s.id, editTitle.trim())
                      }
                      if (e.key === 'Escape') setEditingId(null)
                    }}
                    onBlur={() => renameSession(s.id, editTitle.trim())}
                    className="flex-1 min-w-0 px-2 py-1 text-[15px] font-medium bg-gray-100 rounded-lg border-none outline-none focus:ring-2 focus:ring-blue-300"
                  />
                </div>
              ) : (
                <div
                  className="flex-1 min-w-0 cursor-pointer"
                  onClick={() => navigate(`/chat?sid=${s.id}&lang=${s.language || ''}`)}
                >
                  <div className="font-medium text-[15px] truncate">{s.title || getLangLabel(s.language) || 'Untitled'}</div>
                  <div className="text-xs text-gray-400 truncate">
                    {s.language && <span className="mr-1">{getLangLabel(s.language)}</span>}
                    Last: {timeAgo(s.last_activity)}
                  </div>
                </div>
              )}

              {editingId !== s.id && (
                <div className="flex items-center gap-1 shrink-0">
                  <button
                    onClick={() => startEdit(s)}
                    className="w-8 h-8 flex items-center justify-center rounded-full hover:bg-gray-200 active:bg-gray-300 transition-colors text-gray-400 hover:text-gray-600"
                    title="Rename"
                  >
                    <EditOutlined className="text-xs" />
                  </button>
                  <button
                    onClick={() => setDeletingId(s.id)}
                    className="w-8 h-8 flex items-center justify-center rounded-full hover:bg-red-50 active:bg-red-100 transition-colors text-gray-400 hover:text-red-500"
                    title="Delete"
                  >
                    <DeleteOutlined className="text-xs" />
                  </button>
                </div>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  )
}
