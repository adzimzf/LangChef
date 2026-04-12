import { useState, useEffect } from 'react'
import { invoke } from '@tauri-apps/api/core'
import {
  ApiOutlined,
  KeyOutlined,
  CheckCircleOutlined,
  ExclamationCircleOutlined,
  LoadingOutlined,
  SaveOutlined,
  ExperimentOutlined,
  InfoCircleOutlined,
  BookOutlined,
} from '@ant-design/icons'

const MAX_REVIEWS_DEFAULT = 100
const MAX_REVIEWS_KEY = 'max_reviews_per_day'

export default function Settings() {
  const [apiHost, setApiHost] = useState('')
  const [apiKey, setApiKey] = useState('')
  const [model, setModel] = useState('')
  const [models, setModels] = useState<string[]>([])
  const [status, setStatus] = useState<'idle' | 'saving' | 'saved' | 'testing' | 'ok' | 'error'>('idle')
  const [maxReviews, setMaxReviews] = useState(MAX_REVIEWS_DEFAULT)

  useEffect(() => {
    const host = localStorage.getItem('api_host') || ''
    const key = localStorage.getItem('api_key') || ''
    const savedModel = localStorage.getItem('api_model') || ''
    const savedMax = localStorage.getItem(MAX_REVIEWS_KEY)
    setApiHost(host)
    setApiKey(key)
    setModel(savedModel)
    setMaxReviews(savedMax ? parseInt(savedMax, 10) || MAX_REVIEWS_DEFAULT : MAX_REVIEWS_DEFAULT)
  }, [])

  function save() {
    localStorage.setItem('api_host', apiHost.trim())
    localStorage.setItem('api_key', apiKey.trim())
    localStorage.setItem('api_model', model.trim())
    localStorage.setItem(MAX_REVIEWS_KEY, String(maxReviews))
    setStatus('saved')
    setTimeout(() => setStatus('idle'), 2000)
  }

  async function test() {
    setStatus('testing')
    try {
      const res = await invoke<string>('test_provider', { apiHost, apiKey })
      if (res === 'ok') {
        // Fetch models on success
        try {
          const modelList = await invoke<string[]>('list_models', { apiHost, apiKey })
          setModels(modelList)
          // Auto-select first model if none selected
          if (!model && modelList.length > 0) {
            setModel(modelList[0])
          }
        } catch {
          setModels([])
        }
        setStatus('ok')
      } else {
        setStatus('error')
      }
    } catch (e) {
      setStatus('error')
    }
    setTimeout(() => setStatus('idle'), 3000)
  }

  const statusConfig = {
    idle: null,
    saving: { icon: <LoadingOutlined />, text: 'Saving...', color: 'text-blue-500', bg: 'bg-blue-50' },
    saved: { icon: <CheckCircleOutlined />, text: 'Saved', color: 'text-green-600', bg: 'bg-green-50' },
    testing: { icon: <LoadingOutlined />, text: 'Testing...', color: 'text-blue-500', bg: 'bg-blue-50' },
    ok: { icon: <CheckCircleOutlined />, text: 'Connection OK', color: 'text-green-600', bg: 'bg-green-50' },
    error: { icon: <ExclamationCircleOutlined />, text: 'Connection failed', color: 'text-red-500', bg: 'bg-red-50' },
  }

  const currentStatus = statusConfig[status]

  return (
    <div className="space-y-5 h-full overflow-y-auto">
      <div className="sticky top-0 z-10 pt-4 pb-2 bg-gray-50">
        <h2 className="text-lg font-semibold">Settings</h2>
      </div>

      {/* Provider section */}
      <div className="space-y-3">
        <h3 className="text-sm font-medium text-gray-500 uppercase tracking-wide px-1">API Provider</h3>

        <div className="bg-white rounded-xl shadow-sm overflow-hidden">
          {/* API Host */}
          <div className="px-4 py-3 border-b border-gray-50">
            <label className="flex items-center gap-2 text-sm text-gray-500 mb-1.5">
              <ApiOutlined />
              API Host
            </label>
            <input
              type="text"
              placeholder="https://api.openai.com/v1"
              value={apiHost}
              onChange={(e) => setApiHost(e.target.value)}
              className="w-full text-[15px] bg-transparent outline-none placeholder:text-gray-300"
            />
          </div>

          {/* API Key */}
          <div className="px-4 py-3 border-b border-gray-50">
            <label className="flex items-center gap-2 text-sm text-gray-500 mb-1.5">
              <KeyOutlined />
              API Key
            </label>
            <input
              type="password"
              placeholder="sk-..."
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              className="w-full text-[15px] bg-transparent outline-none placeholder:text-gray-300"
            />
          </div>

          {/* Model selection */}
          <div className="px-4 py-3">
            <label className="flex items-center gap-2 text-sm text-gray-500 mb-1.5">
              <ApiOutlined />
              Model
            </label>
            {models.length > 0 ? (
              <select
                value={model}
                onChange={(e) => setModel(e.target.value)}
                className="w-full text-[15px] bg-transparent outline-none cursor-pointer"
              >
                {models.map((m) => (
                  <option key={m} value={m}>{m}</option>
                ))}
              </select>
            ) : (
              <div className="flex items-center gap-2">
                <input
                  type="text"
                  placeholder="gpt-3.5-turbo"
                  value={model}
                  onChange={(e) => setModel(e.target.value)}
                  className="w-full text-[15px] bg-transparent outline-none placeholder:text-gray-300"
                />
              </div>
            )}
            {models.length === 0 && (
              <p className="text-xs text-gray-400 mt-1">Test connection to load available models</p>
            )}
          </div>
        </div>

        {/* Action buttons */}
        <div className="flex gap-2">
          <button
            onClick={save}
            className="flex-1 flex items-center justify-center gap-2 py-3 rounded-xl bg-blue-500 text-white font-medium text-sm hover:bg-blue-600 active:bg-blue-700 transition-colors"
          >
            <SaveOutlined />
            Save
          </button>
          <button
            onClick={test}
            className="flex-1 flex items-center justify-center gap-2 py-3 rounded-xl bg-gray-100 text-gray-700 font-medium text-sm hover:bg-gray-200 active:bg-gray-300 transition-colors"
          >
            <ExperimentOutlined />
            Test
          </button>
        </div>

        {/* Status indicator */}
        {currentStatus && (
          <div className={`flex items-center gap-2 px-4 py-2.5 rounded-xl ${currentStatus.bg} ${currentStatus.color} text-sm font-medium`}>
            {currentStatus.icon}
            {currentStatus.text}
          </div>
        )}
      </div>

      {/* Info section */}
      <div className="bg-gray-50 rounded-xl p-4">
        <div className="flex items-start gap-2.5">
          <InfoCircleOutlined className="text-gray-400 mt-0.5" />
          <p className="text-sm text-gray-500 leading-relaxed">
            Your API key is stored locally and only used in-memory for chat requests and connection tests. It is never sent to any server other than your configured API host.
          </p>
        </div>
      </div>

      {/* Review settings */}
      <div className="space-y-3">
        <h3 className="text-sm font-medium text-gray-500 uppercase tracking-wide px-1">Review</h3>

        <div className="bg-white rounded-xl shadow-sm overflow-hidden">
          <div className="px-4 py-3">
            <label className="flex items-center gap-2 text-sm text-gray-500 mb-1.5">
              <BookOutlined />
              Max reviews per day
            </label>
            <input
              type="number"
              min={1}
              max={999}
              value={maxReviews}
              onChange={(e) => setMaxReviews(Math.max(1, parseInt(e.target.value) || MAX_REVIEWS_DEFAULT))}
              className="w-full text-[15px] bg-transparent outline-none placeholder:text-gray-300"
            />
            <p className="text-xs text-gray-400 mt-1">
              Prevent burnout by limiting daily reviews. Anki default: 100. Current: {maxReviews}
            </p>
          </div>
        </div>
      </div>
    </div>
  )
}
