import { useEffect, useRef, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { PlusOutlined, DeleteOutlined, EditOutlined, CheckOutlined, CloseOutlined, ReadOutlined, UploadOutlined } from '@ant-design/icons'
import ReactMarkdown from 'react-markdown'
import { JLPT_LEVEL_ORDER, DEFAULT_JLPT_LEVEL } from '../lib/jlpt'

type GrammarItem = {
  id: string
  pattern: string
  detail: string
  language: string
  level: string
  is_default: number
}

const LANGUAGES = [
  { code: 'ja', label: 'Japanese', flag: '\u{1F1EF}\u{1F1F5}' },
  { code: 'id', label: 'Indonesian', flag: '\u{1F1EE}\u{1F1E9}' },
]

// Fallback levels per language when DB is empty
const FALLBACK_LEVELS: Record<string, string[]> = {
  ja: JLPT_LEVEL_ORDER,
  id: ['A1', 'A2'],
}

export default function Grammar() {
  const [items, setItems] = useState<GrammarItem[]>([])
  const [selectedLang, setSelectedLang] = useState('ja')
  const [selectedLevel, setSelectedLevel] = useState<string>(DEFAULT_JLPT_LEVEL)
  const [editingId, setEditingId] = useState<string | null>(null)
  const [editPattern, setEditPattern] = useState('')
  const [editDetail, setEditDetail] = useState('')
  const [adding, setAdding] = useState(false)
  const [addPattern, setAddPattern] = useState('')
  const [addDetail, setAddDetail] = useState('')
  const [deletingId, setDeletingId] = useState<string | null>(null)
  const [dbLevels, setDbLevels] = useState<string[]>(FALLBACK_LEVELS['ja'])
  const [showImport, setShowImport] = useState(false)
  const [importText, setImportText] = useState('')
  const [importLevel, setImportLevel] = useState('')
  const [expandedId, setExpandedId] = useState<string | null>(null)
  const fileInputRef = useRef<HTMLInputElement>(null)

  async function load() {
    try {
      const res = await invoke<GrammarItem[]>('db_list_grammar_items', { language: selectedLang, level: selectedLevel })
      setItems(res)
    } catch (e) {
      console.error('Failed to load grammar items:', e)
    }
  }

  async function loadLevels() {
    try {
      const levels = await invoke<string[]>('db_get_grammar_levels', { language: selectedLang })
      setDbLevels(levels.length > 0 ? levels : (FALLBACK_LEVELS[selectedLang] || []))
    } catch {
      setDbLevels(FALLBACK_LEVELS[selectedLang] || [])
    }
  }

  // Load levels and items on mount and when language/level changes
  useEffect(() => {
    loadLevels()
    load()
  }, [selectedLang, selectedLevel])

  // Reset level when language changes
  useEffect(() => {
    const levels = FALLBACK_LEVELS[selectedLang] || []
    if (!levels.includes(selectedLevel)) {
      setSelectedLevel(levels[0] || '')
    }
  }, [selectedLang])

  async function addItem() {
    if (!addPattern.trim() || !addDetail.trim()) return
    try {
      await invoke('db_add_grammar_item', {
        pattern: addPattern.trim(),
        detail: addDetail.trim(),
        language: selectedLang,
        level: selectedLevel,
      })
      setAddPattern('')
      setAddDetail('')
      setAdding(false)
      load()
      loadLevels()
    } catch (e) {
      console.error('Failed to add grammar item:', e)
    }
  }

  async function saveEdit(id: string) {
    if (!editPattern.trim()) return
    try {
      await invoke('db_update_grammar_item', {
        id,
        pattern: editPattern.trim(),
        detail: editDetail.trim(),
      })
      setEditingId(null)
      load()
    } catch (e) {
      console.error('Failed to update grammar item:', e)
    }
  }

  async function deleteItem(id: string) {
    try {
      await invoke('db_delete_grammar_item', { id })
      setDeletingId(null)
      load()
      loadLevels()
    } catch (e) {
      console.error('Failed to delete grammar item:', e)
    }
  }

  function startEdit(item: GrammarItem) {
    setEditingId(item.id)
    setEditPattern(item.pattern)
    setEditDetail(item.detail)
    setAdding(false)
  }

  // CSV import
  async function handleImport() {
    const lines = importText.trim().split('\n').filter(l => l.trim())
    if (lines.length === 0) return

    const itemsToImport: { pattern: string; detail: string; language: string; level: string }[] = []
    for (const line of lines) {
      // Support both comma and tab separated
      const sep = line.includes('\t') ? '\t' : ','
      const parts = line.split(sep)
      if (parts.length >= 2) {
        itemsToImport.push({
          pattern: parts[0].trim(),
          detail: parts.slice(1).join(sep).trim(),
          language: selectedLang,
          level: importLevel || selectedLevel,
        })
      }
    }
    if (itemsToImport.length === 0) return

    try {
      await invoke('db_import_grammar_csv', { items: itemsToImport })
      setShowImport(false)
      setImportText('')
      setImportLevel('')
      load()
      loadLevels()
    } catch (e) {
      console.error('Failed to import grammar:', e)
    }
  }

  function handleFileUpload(e: React.ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0]
    if (!file) return
    const reader = new FileReader()
    reader.onload = (ev) => {
      const text = ev.target?.result as string
      setImportText(text)
    }
    reader.readAsText(file)
    // Reset the input so the same file can be re-uploaded
    e.target.value = ''
  }

  const levels = dbLevels

  return (
    <div className="flex flex-col h-full overflow-y-auto">
      {/* Sticky header */}
      <div className="sticky top-0 z-10 py-3 bg-gray-50 shrink-0">
        <h2 className="text-lg font-semibold">Grammar</h2>
      </div>

      {/* Language & Level picker */}
      <div className="flex gap-2 mb-3 shrink-0">
        <div className="flex gap-1 bg-white rounded-xl p-1 shadow-sm">
          {LANGUAGES.map(lang => (
            <button
              key={lang.code}
              onClick={() => setSelectedLang(lang.code)}
              className={`px-3 py-1.5 rounded-lg text-sm font-medium transition-colors ${
                selectedLang === lang.code
                  ? 'bg-blue-500 text-white'
                  : 'text-gray-600 hover:bg-gray-100'
              }`}
            >
              {lang.flag} {lang.label}
            </button>
          ))}
        </div>
      </div>

      <div className="flex gap-1.5 mb-4 overflow-x-auto no-scrollbar shrink-0">
        {levels.map(level => (
          <button
            key={level}
            onClick={() => setSelectedLevel(level)}
            className={`shrink-0 px-3 py-1.5 rounded-full text-sm font-medium transition-colors ${
              selectedLevel === level
                ? 'bg-indigo-500 text-white'
                : 'bg-white text-gray-600 hover:bg-indigo-50 active:bg-indigo-100'
            }`}
          >
            {level}
          </button>
        ))}
      </div>

      {/* Action buttons */}
      <div className="flex gap-2 mb-3 shrink-0">
        {!adding && !showImport && (
          <>
            <button
              onClick={() => { setAdding(true); setEditingId(null); setShowImport(false) }}
              className="flex items-center gap-2 px-4 py-2 rounded-xl bg-blue-500 text-white text-sm font-medium hover:bg-blue-600 active:bg-blue-700 transition-colors"
            >
              <PlusOutlined />
              Add
            </button>
            <button
              onClick={() => { setShowImport(true); setAdding(false); setEditingId(null) }}
              className="flex items-center gap-2 px-4 py-2 rounded-xl bg-white text-gray-700 text-sm font-medium shadow-sm hover:bg-gray-50 active:bg-gray-100 transition-colors border border-gray-200"
            >
              <UploadOutlined />
              Import CSV
            </button>
          </>
        )}
      </div>

      {/* Add form */}
      {adding && (
        <div className="bg-white rounded-xl p-4 shadow-sm space-y-3 mb-3 shrink-0">
          <div className="flex items-center gap-2">
            <span className="text-xs font-medium text-gray-500 w-16 shrink-0">Pattern</span>
            <input
              autoFocus
              value={addPattern}
              onChange={e => setAddPattern(e.target.value)}
              placeholder="e.g. んです"
              className="flex-1 px-3 py-2 bg-gray-50 rounded-lg text-sm border-none outline-none focus:ring-2 focus:ring-blue-300"
            />
          </div>
          <div className="flex items-start gap-2">
            <span className="text-xs font-medium text-gray-500 w-16 shrink-0 pt-2">Detail</span>
            <textarea
              value={addDetail}
              onChange={e => setAddDetail(e.target.value)}
              placeholder="Markdown supported. e.g. **んです** is used for explanation..."
              rows={4}
              className="flex-1 px-3 py-2 bg-gray-50 rounded-lg text-sm border-none outline-none focus:ring-2 focus:ring-blue-300 resize-none font-mono"
            />
          </div>
          <div className="flex gap-2 justify-end">
            <button
              onClick={() => { setAdding(false); setAddPattern(''); setAddDetail('') }}
              className="px-3 py-1.5 rounded-lg text-sm text-gray-500 hover:bg-gray-100 active:bg-gray-200 transition-colors"
            >
              Cancel
            </button>
            <button
              onClick={addItem}
              disabled={!addPattern.trim() || !addDetail.trim()}
              className="px-4 py-1.5 rounded-lg text-sm font-medium bg-blue-500 text-white hover:bg-blue-600 active:bg-blue-700 disabled:bg-gray-200 disabled:text-gray-400 transition-colors"
            >
              Add
            </button>
          </div>
        </div>
      )}

      {/* Import form */}
      {showImport && (
        <div className="bg-white rounded-xl p-4 shadow-sm space-y-3 mb-3 shrink-0">
          <div className="flex items-center justify-between">
            <h4 className="text-sm font-semibold">Import Grammar (CSV)</h4>
            <button onClick={() => { setShowImport(false); setImportText('') }} className="text-gray-400 hover:text-gray-600">
              <CloseOutlined />
            </button>
          </div>
          <p className="text-xs text-gray-500">
            Format: one item per line. <code className="bg-gray-100 px-1 rounded">pattern,detail</code> or tab-separated.
            The detail field supports Markdown.
          </p>
          <div className="flex items-center gap-2">
            <span className="text-xs font-medium text-gray-500 w-16 shrink-0">Level</span>
            <select
              value={importLevel || selectedLevel}
              onChange={e => setImportLevel(e.target.value)}
              className="flex-1 px-3 py-2 bg-gray-50 rounded-lg text-sm border-none outline-none focus:ring-2 focus:ring-blue-300"
            >
              {levels.map(l => <option key={l} value={l}>{l}</option>)}
            </select>
          </div>
          <div className="flex gap-2">
            <button
              onClick={() => fileInputRef.current?.click()}
              className="flex items-center gap-2 px-3 py-2 rounded-lg text-sm text-gray-600 bg-gray-50 hover:bg-gray-100 transition-colors"
            >
              <UploadOutlined /> Choose file
            </button>
            <input
              ref={fileInputRef}
              type="file"
              accept=".csv,.tsv,.txt"
              className="hidden"
              onChange={handleFileUpload}
            />
          </div>
          <textarea
            value={importText}
            onChange={e => setImportText(e.target.value)}
            placeholder={`Example:\nんです,Used for giving explanations or seeking confirmation\nばかり,Just did something / nothing but`}
            rows={5}
            className="w-full px-3 py-2 bg-gray-50 rounded-lg text-sm border-none outline-none focus:ring-2 focus:ring-blue-300 resize-none font-mono"
          />
          <div className="flex gap-2 justify-end">
            <button
              onClick={() => { setShowImport(false); setImportText('') }}
              className="px-3 py-1.5 rounded-lg text-sm text-gray-500 hover:bg-gray-100 active:bg-gray-200 transition-colors"
            >
              Cancel
            </button>
            <button
              onClick={handleImport}
              disabled={!importText.trim()}
              className="px-4 py-1.5 rounded-lg text-sm font-medium bg-blue-500 text-white hover:bg-blue-600 active:bg-blue-700 disabled:bg-gray-200 disabled:text-gray-400 transition-colors"
            >
              Import
            </button>
          </div>
        </div>
      )}

      {/* Grammar list */}
      <div className="space-y-2 pb-4">
        {items.length === 0 && (
          <div className="text-center text-gray-400 py-8">
            <ReadOutlined className="text-3xl mb-2" />
            <p className="text-sm">No grammar items for this level</p>
          </div>
        )}
        {items.map(item => (
          <div
            key={item.id}
            className="bg-white rounded-xl shadow-sm overflow-hidden"
          >
            {editingId === item.id ? (
              /* Edit mode */
              <div className="p-4 space-y-3">
                <div className="flex items-center gap-2">
                  <span className="text-xs font-medium text-gray-500 w-16 shrink-0">Pattern</span>
                  <input
                    autoFocus
                    value={editPattern}
                    onChange={e => setEditPattern(e.target.value)}
                    className="flex-1 px-3 py-2 bg-gray-50 rounded-lg text-sm border-none outline-none focus:ring-2 focus:ring-blue-300"
                  />
                </div>
                <div className="flex items-start gap-2">
                  <span className="text-xs font-medium text-gray-500 w-16 shrink-0 pt-2">Detail</span>
                  <textarea
                    value={editDetail}
                    onChange={e => setEditDetail(e.target.value)}
                    rows={5}
                    className="flex-1 px-3 py-2 bg-gray-50 rounded-lg text-sm border-none outline-none focus:ring-2 focus:ring-blue-300 resize-none font-mono"
                  />
                </div>
                <div className="flex gap-2 justify-end">
                  <button
                    onClick={() => setEditingId(null)}
                    className="px-3 py-1.5 rounded-lg text-sm text-gray-500 hover:bg-gray-100 active:bg-gray-200 transition-colors"
                  >
                    <CloseOutlined className="mr-1" />Cancel
                  </button>
                  <button
                    onClick={() => saveEdit(item.id)}
                    disabled={!editPattern.trim()}
                    className="px-4 py-1.5 rounded-lg text-sm font-medium bg-blue-500 text-white hover:bg-blue-600 disabled:bg-gray-200 disabled:text-gray-400 transition-colors"
                  >
                    <CheckOutlined className="mr-1" />Save
                  </button>
                </div>
              </div>
            ) : (
              /* View mode */
              <div className="p-4">
                <div className="flex items-start justify-between gap-2">
                  <div
                    className="flex-1 min-w-0 cursor-pointer"
                    onClick={() => setExpandedId(expandedId === item.id ? null : item.id)}
                  >
                    <div className="flex items-center gap-2 mb-1">
                      <span className="font-semibold text-[15px] text-gray-800">{item.pattern}</span>
                      {item.is_default ? (
                        <span className="text-[10px] px-1.5 py-0.5 rounded-full bg-gray-100 text-gray-400 font-medium">default</span>
                      ) : (
                        <span className="text-[10px] px-1.5 py-0.5 rounded-full bg-blue-50 text-blue-400 font-medium">custom</span>
                      )}
                    </div>
                    {expandedId === item.id ? (
                      <div className="grammar-detail prose prose-sm max-w-none text-gray-600 mt-2">
                        <ReactMarkdown>{item.detail}</ReactMarkdown>
                      </div>
                    ) : (
                      <p className="text-sm text-gray-500 leading-relaxed line-clamp-2">{item.detail}</p>
                    )}
                  </div>
                  <div className="flex items-center gap-1 shrink-0">
                    <button
                      onClick={() => startEdit(item)}
                      className="w-8 h-8 flex items-center justify-center rounded-full hover:bg-gray-100 active:bg-gray-200 transition-colors text-gray-400 hover:text-gray-600"
                      title="Edit"
                    >
                      <EditOutlined className="text-xs" />
                    </button>
                    <button
                      onClick={() => setDeletingId(item.id)}
                      className="w-8 h-8 flex items-center justify-center rounded-full hover:bg-red-50 active:bg-red-100 transition-colors text-gray-400 hover:text-red-500"
                      title="Delete"
                    >
                      <DeleteOutlined className="text-xs" />
                    </button>
                  </div>
                </div>
              </div>
            )}
          </div>
        ))}
      </div>

      {/* Delete confirmation */}
      {deletingId && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/30" onClick={() => setDeletingId(null)}>
          <div className="bg-white rounded-2xl p-5 mx-6 max-w-sm w-full" onClick={e => e.stopPropagation()}>
            <h3 className="text-base font-semibold mb-2">Delete Grammar</h3>
            <p className="text-sm text-gray-500 mb-4">This will permanently remove this grammar item. This cannot be undone.</p>
            <div className="flex gap-2 justify-end">
              <button
                onClick={() => setDeletingId(null)}
                className="px-4 py-2 rounded-xl text-sm font-medium hover:bg-gray-100 active:bg-gray-200 transition-colors"
              >
                Cancel
              </button>
              <button
                onClick={() => deleteItem(deletingId)}
                className="px-4 py-2 rounded-xl text-sm font-medium bg-red-500 text-white hover:bg-red-600 active:bg-red-700 transition-colors"
              >
                Delete
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
