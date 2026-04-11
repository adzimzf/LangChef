import { useLocation, useNavigate } from 'react-router-dom'
import { useMemo } from 'react'
import {
  HomeOutlined,
  MessageOutlined,
  BookOutlined,
  BarChartOutlined,
  SettingOutlined,
} from '@ant-design/icons'

const NAV_ITEMS = [
  { key: 'home', label: 'Home', icon: HomeOutlined, path: '/' },
  { key: 'chats', label: 'Chats', icon: MessageOutlined, path: '/chats' },
  { key: 'review', label: 'Review', icon: BookOutlined, path: '/review' },
  { key: 'telemetry', label: 'Stats', icon: BarChartOutlined, path: '/telemetry' },
  { key: 'settings', label: 'Settings', icon: SettingOutlined, path: '/settings' },
]

export default function Layout({ children }: { children: React.ReactNode }) {
  const location = useLocation()
  const navigate = useNavigate()
  const isChatPage = location.pathname === '/chat'

  const activeKey = useMemo(() => {
    if (location.pathname.startsWith('/chats')) return 'chats'
    if (location.pathname.startsWith('/chat')) return 'chats' // chat falls under chats tab
    if (location.pathname.startsWith('/review')) return 'review'
    if (location.pathname.startsWith('/telemetry')) return 'telemetry'
    if (location.pathname.startsWith('/settings')) return 'settings'
    return 'home'
  }, [location.pathname])

  return (
    <div className="min-h-screen bg-gray-50 flex flex-col">
      {/* Header */}
      {!isChatPage && (
        <header className="shrink-0 bg-white border-b border-gray-100 px-4 h-12 flex items-center safe-top">
          <h1 className="text-lg font-bold tracking-tight">LangChef</h1>
        </header>
      )}

      {/* Content */}
      <main className={`flex-1 ${isChatPage ? '' : 'p-4 pb-24'}`}>{children}</main>

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
