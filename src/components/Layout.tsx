import { useLocation, useNavigate } from 'react-router-dom'
import { useMemo, useEffect, useState } from 'react'
import {
  HomeOutlined,
  MessageOutlined,
  BookOutlined,
  BarChartOutlined,
  SettingOutlined,
  FileTextOutlined,
  PartitionOutlined,
  LoadingOutlined,
  CheckCircleOutlined,
  WarningOutlined,
} from '@ant-design/icons'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useActiveTimeHeartbeat } from '../hooks/useActiveTimeHeartbeat'
import { t } from '../lib/i18n'

type LinderaStage = 'idle' | 'downloading' | 'extracting' | 'building' | 'ready' | 'error'

interface LinderaProgress {
  stage: string
  message: string
  percent: number
}

const NAV_ITEMS = [
  { key: 'home', label: t('nav.home'), icon: HomeOutlined, path: '/' },
  { key: 'chats', label: t('nav.chats'), icon: MessageOutlined, path: '/chats' },
  { key: 'reading', label: t('nav.reading'), icon: FileTextOutlined, path: '/reading' },
  { key: 'breakdown', label: t('nav.breakdown'), icon: PartitionOutlined, path: '/breakdown' },
  { key: 'review', label: t('nav.review'), icon: BookOutlined, path: '/review' },
  { key: 'settings', label: t('nav.settings'), icon: SettingOutlined, path: '/settings' },
]

export default function Layout({ children }: { children: React.ReactNode }) {
  const location = useLocation()
  const navigate = useNavigate()
  const isChatPage = location.pathname === '/chat'

  useActiveTimeHeartbeat(true)

  const [linderaStage, setLinderaStage] = useState<LinderaStage>('idle')
  const [linderaMessage, setLinderaMessage] = useState('')
  const [linderaPercent, setLinderaPercent] = useState(0)

  useEffect(() => {
    // Listen for progress events emitted by Rust during download/build
    const unlisten = listen<LinderaProgress>('lindera-progress', (event) => {
      const { stage, message, percent } = event.payload
      setLinderaStage(stage as LinderaStage)
      const key = `lindera.${stage}`
      setLinderaMessage(t(key) === key ? message : t(key))
      setLinderaPercent(percent)
    })

    // Trigger init — no-op if dict already cached; otherwise starts download
    // Once Japanese can be split into words, fold cards that are forms of the same word (下がります → 下がる).
    invoke<string>('init_lindera_dict').then(() => { invoke('db_merge_duplicate_words').catch(() => {}) }).catch((err) => {
      setLinderaStage('error')
      setLinderaMessage(`${t('lindera.error')} ${String(err)}`)
    })

    return () => {
      unlisten.then((fn) => fn())
    }
  }, [])

  const showBanner = linderaStage !== 'idle' && linderaStage !== 'ready'
  const isActive = linderaStage === 'downloading' || linderaStage === 'extracting' || linderaStage === 'building'

  const activeKey = useMemo(() => {
    if (location.pathname.startsWith('/chats')) return 'chats'
    if (location.pathname.startsWith('/chat')) return 'chats'
    if (location.pathname.startsWith('/reading')) return 'reading'
    if (location.pathname.startsWith('/breakdown')) return 'breakdown'
    if (location.pathname.startsWith('/review')) return 'review'
    if (location.pathname.startsWith('/settings')) return 'settings'
    if (location.pathname.startsWith('/stats')) return 'home'
    if (location.pathname.startsWith('/telemetry')) return 'home'
    if (location.pathname.startsWith('/words')) return 'home'
    return 'home'
  }, [location.pathname])

  return (
    <div className="h-dvh bg-gray-50 flex flex-col">
      {/* Content */}
      <main className={`flex-1 overflow-hidden ${isChatPage ? '' : 'px-4 pb-[50px]'}`}>{children}</main>

      {/* Lindera dict download/build progress banner */}
      {showBanner && (
        <div className={`fixed bottom-14 left-0 right-0 z-40 px-3 py-2 text-xs flex items-center gap-2
          ${linderaStage === 'error' ? 'bg-red-50 text-red-700 border-t border-red-200' : 'bg-blue-50 text-blue-700 border-t border-blue-200'}`}>
          <span className="shrink-0">
            {linderaStage === 'error'
              ? <WarningOutlined />
              : isActive
                ? <LoadingOutlined spin />
                : <CheckCircleOutlined />}
          </span>
          <span className="flex-1 truncate">{linderaMessage}</span>
          {isActive && linderaPercent > 0 && (
            <span className="shrink-0 font-medium">{linderaPercent}%</span>
          )}
        </div>
      )}

      {/* Bottom navigation */}
      {!isChatPage && (
        <nav className="shrink-0 fixed bottom-0 left-0 right-0 bg-white border-t border-gray-100 safe-bottom z-50">
          <div className="flex items-center justify-around h-14 max-w-lg mx-auto">
            {NAV_ITEMS.map((item) => {
              const isActive = activeKey === item.key
              const Icon = item.icon
              return (
                <button
                  key={item.key}
                  onClick={() => navigate(item.path)}
                  className={`
                    flex flex-col items-center justify-center gap-0.5 flex-1 h-full transition-colors
                    ${isActive ? 'text-blue-500' : 'text-gray-400 active:text-gray-600'}
                  `}
                >
                  <Icon className="text-xl" style={{ fontSize: 22 }} />
                  <span className={`text-[10px] leading-tight ${isActive ? 'font-semibold' : ''}`}>
                    {item.label}
                  </span>
                </button>
              )
            })}
          </div>
        </nav>
      )}
    </div>
  )
}
