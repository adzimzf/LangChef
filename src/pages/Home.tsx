import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { invoke } from '@tauri-apps/api/core'
import {
  MessageOutlined,
  BookOutlined,
  FireOutlined,
  ThunderboltOutlined,
  RightOutlined,
} from '@ant-design/icons'
import { type Day, todayKey, computeStreak } from '../lib/stats'

export default function Home() {
  const navigate = useNavigate()

  const [stats, setStats] = useState({
    streak: 0,
    mastered: 0,
    total: 0,
    sessionsToday: 0,
    knownWords: 0,
  })

  useEffect(() => {
    async function loadStats() {
      try {
        const [telemetryDays, reviewItems] = await Promise.all([
          invoke<Day[]>('db_get_telemetry_days'),
          invoke<{ R: number }[]>('db_get_review_items'),
        ])
        const streak = computeStreak(telemetryDays)
        const mastered = reviewItems.filter((c) => c.R >= 3).length
        const sessionsToday = telemetryDays.find((d) => d.date === todayKey())?.sessions ?? 0

        setStats({
          streak,
          mastered,
          total: reviewItems.length,
          sessionsToday,
          knownWords: mastered,
        })
      } catch (e) {
        console.error('Failed to load stats:', e)
      }
    }
    loadStats()
  }, [])

  const actions = [
    {
      key: 'chat',
      icon: <MessageOutlined className="text-xl" />,
      label: 'Start Chatting',
      desc: 'Practice your target language',
      color: 'bg-blue-500',
      onClick: () => navigate('/chats'),
    },
    {
      key: 'review',
      icon: <BookOutlined className="text-xl" />,
      label: 'Review Words',
      desc: `${stats.mastered}/${stats.total} mastered`,
      color: 'bg-green-500',
      onClick: () => navigate('/review'),
    },
    {
      key: 'telemetry',
      icon: <ThunderboltOutlined className="text-xl" />,
      label: 'Your Stats',
      desc: `${stats.sessionsToday} sessions today`,
      color: 'bg-purple-500',
      onClick: () => navigate('/telemetry'),
    },
  ]

  return (
    <div className="space-y-5">
      {/* Hero section */}
      <div className="bg-gradient-to-br from-blue-500 to-blue-600 rounded-2xl p-5 text-white">
        <h2 className="text-xl font-bold mb-1">LangChef</h2>
        <p className="text-blue-100 text-sm mb-4">Your daily language practice</p>
        <div className="flex gap-4">
          <div className="flex items-center gap-2">
            <FireOutlined className="text-orange-300 text-lg" />
            <span className="font-semibold text-lg">{stats.streak}</span>
            <span className="text-blue-200 text-sm">day streak</span>
          </div>
          <div className="flex items-center gap-2">
            <BookOutlined className="text-green-300 text-lg" />
            <span className="font-semibold text-lg">{stats.knownWords}</span>
            <span className="text-blue-200 text-sm">words</span>
          </div>
        </div>
      </div>

      {/* Quick actions */}
      <div className="space-y-2">
        <h3 className="text-sm font-medium text-gray-500 uppercase tracking-wide px-1">Quick Actions</h3>
        {actions.map((a) => (
          <button
            key={a.key}
            onClick={a.onClick}
            className="w-full flex items-center gap-3 bg-white rounded-xl p-4 shadow-sm hover:shadow active:bg-gray-50 transition-all text-left"
          >
            <div className={`w-11 h-11 rounded-xl ${a.color} flex items-center justify-center text-white shrink-0`}>
              {a.icon}
            </div>
            <div className="flex-1 min-w-0">
              <div className="font-medium text-[15px]">{a.label}</div>
              <div className="text-xs text-gray-400">{a.desc}</div>
            </div>
            <RightOutlined className="text-gray-300" />
          </button>
        ))}
      </div>

      {/* Tip card */}
      <div className="bg-amber-50 border border-amber-100 rounded-xl p-4">
        <p className="text-sm text-amber-800">
          <strong>Tip:</strong> Tap any word in chat replies to mark it as difficult. It'll appear in your review deck!
        </p>
      </div>
    </div>
  )
}
