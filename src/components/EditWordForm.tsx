import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import {
  ArrowLeftOutlined,
  CheckOutlined,
  RobotOutlined,
  BulbOutlined,
} from '@ant-design/icons'
import type { ReviewItem } from '../lib/types'
import { parseDetailJson } from '../lib/types'

interface EditWordFormProps {
  item: ReviewItem
  onSave: (data: { content: string; detail: string; mnemonic: string }) => Promise<void>
  onCancel: () => void
  title?: string
}

export default function EditWordForm({ item, onSave, onCancel, title = 'Edit Word' }: EditWordFormProps) {
  const [editContent, setEditContent] = useState('')
  const [editDetail, setEditDetail] = useState('')
  const [generatingDetail, setGeneratingDetail] = useState(false)
  const [editReading, setEditReading] = useState('')
  const [editMeaning, setEditMeaning] = useState('')
  const [editPartOfSpeech, setEditPartOfSpeech] = useState('')
  const [editExample, setEditExample] = useState('')
  const [editExampleMeaning, setEditExampleMeaning] = useState('')
  const [editMnemonic, setEditMnemonic] = useState('')
  const [generatingMnemonic, setGeneratingMnemonic] = useState(false)
  const [saveError, setSaveError] = useState<string | null>(null)

  // Initialize form fields from item on mount / when item changes
  useEffect(() => {
    setEditContent(item.content)
    setEditDetail(item.detail || '')
    const parsed = item.detail ? parseDetailJson(item.detail) : null
    setEditReading(parsed?.reading || '')
    setEditMeaning(parsed?.meaning || '')
    setEditPartOfSpeech(parsed?.part_of_speech || '')
    setEditExample(parsed?.example || '')
    setEditExampleMeaning(parsed?.example_meaning || '')
    setEditMnemonic(item.mnemonic || '')
  }, [item])

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
    const apiHost = localStorage.getItem('api_host') || ''
    const apiKey = localStorage.getItem('api_key') || ''
    const model = localStorage.getItem('api_vocab_model') || localStorage.getItem('api_model') || 'gpt-3.5-turbo'
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
      const parsed = parseDetailJson(result)
      setEditReading(parsed?.reading || '')
      setEditMeaning(parsed?.meaning || '')
      setEditPartOfSpeech(parsed?.part_of_speech || '')
      setEditExample(parsed?.example || '')
      setEditExampleMeaning(parsed?.example_meaning || '')
    } catch (e) {
      alert(`Failed to generate: ${e}`)
    }
    setGeneratingDetail(false)
  }

  async function generateMnemonic() {
    const apiHost = localStorage.getItem('api_host') || ''
    const apiKey = localStorage.getItem('api_key') || ''
    const model = localStorage.getItem('api_vocab_model') || localStorage.getItem('api_model') || 'gpt-3.5-turbo'
    if (!apiHost || !apiKey) {
      alert('Please configure your API settings first (Settings page).')
      return
    }
    setGeneratingMnemonic(true)
    try {
      const lang = item.language || 'ja'
      const result = await invoke<string>('db_generate_mnemonic', {
        apiHost, apiKey, model,
        id: item.id,
        content: item.content,
        language: lang,
        detail: item.detail,
      })
      setEditMnemonic(result)
    } catch (e) {
      alert(`Failed to generate mnemonic: ${e}`)
    }
    setGeneratingMnemonic(false)
  }

  async function handleSave() {
    setSaveError(null)
    try {
      const detailValue = serializeDetail()
      await onSave({
        content: editContent.trim(),
        detail: detailValue,
        mnemonic: editMnemonic,
      })
    } catch (e: any) {
      const msg = typeof e === 'string' ? e : e?.message || String(e)
      setSaveError(msg)
    }
  }

  const hasDetail = editReading || editMeaning || editPartOfSpeech || editExample || editExampleMeaning || editDetail

  return (
    <div className="flex flex-col h-full -m-4">
      <div className="shrink-0 sticky top-0 z-10 bg-gray-50 flex items-center gap-3 px-4 pt-4 pb-2">
        <button
          onClick={onCancel}
          className="w-9 h-9 flex items-center justify-center rounded-full hover:bg-gray-100 transition-colors"
        >
          <ArrowLeftOutlined />
        </button>
        <h2 className="text-lg font-semibold">{title}</h2>
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
              <div>
                <div className="flex items-center justify-between mb-0.5">
                  <label className="text-xs text-gray-400">Mnemonic</label>
                  <button
                    onClick={generateMnemonic}
                    disabled={generatingMnemonic}
                    className="flex items-center gap-1 px-2 py-0.5 rounded-lg bg-amber-50 text-amber-600 text-xs font-medium hover:bg-amber-100 transition-colors disabled:opacity-50"
                  >
                    <BulbOutlined />
                    {generatingMnemonic ? 'Creating...' : 'Generate'}
                  </button>
                </div>
                <textarea
                  value={editMnemonic}
                  onChange={(e) => setEditMnemonic(e.target.value)}
                  className="w-full px-2.5 py-1.5 text-sm border border-gray-200 rounded-lg outline-none focus:border-blue-400 bg-white resize-none"
                  placeholder="A memory aid connecting to words you already know"
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
          onClick={handleSave}
          className="w-full py-3 rounded-xl bg-blue-500 text-white font-medium hover:bg-blue-600 active:bg-blue-700 transition-colors flex items-center justify-center gap-2"
        >
          <CheckOutlined /> Save
        </button>
      </div>
    </div>
  )
}