import { useEffect, useMemo } from 'react'
import { useNavigate } from 'react-router-dom'
import { invoke } from '@tauri-apps/api/core'
import {
  MessageOutlined,
  BookOutlined,
  FireOutlined,
  ThunderboltOutlined,
  RightOutlined,
} from '@ant-design/icons'

type Day = { date: string; sessions: number }

function todayKey() {
  return new Date().toISOString().slice(0, 10)
}

export default function Home() {
  const navigate = useNavigate()

  const stats = useMemo(() => {
    const days: Day[] = JSON.parse(localStorage.getItem('telemetry_days') || '[]')
    const reviewItems = JSON.parse(localStorage.getItem('review_items') || '[]')
    const knownWords: string[] = JSON.parse(localStorage.getItem('known_words') || '[]')

    const streak = (() => {
      const sorted = [...days].sort((a, b) => a.date.localeCompare(b.date))
      let s = 0
      let prev: string | null = null
      for (let i = sorted.length - 1; i >= 0; i--) {
        const d = sorted[i].date
        if (prev == null) {
          if (d === todayKey()) s++
          else break
        } else {
          const diff = (new Date(prev).getTime() - new Date(d).getTime()) / (24 * 60 * 60 * 1000)
          if (diff === 1) s++
          else break
        }
        prev = d
      }
      return s
    })()

    const mastered = reviewItems.filter((c: any) => c.R >= 3).length
    const sessionsToday = days.find((d) => d.date === todayKey())?.sessions ?? 0

    return { streak, mastered, total: reviewItems.length, sessionsToday, knownWords: knownWords.length }
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
