import { useEffect, useMemo, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import {
  FireOutlined,
  ThunderboltOutlined,
  BookOutlined,
  BulbOutlined,
} from '@ant-design/icons'
import { type Day, todayKey, computeStreak, getLastNDays } from '../lib/stats'

export default function Telemetry() {
  const [days, setDays] = useState<Day[]>([])
  const [mastery, setMastery] = useState({ mastered: 0, total: 0 })
  const [knownWords, setKnownWords] = useState(0)

  useEffect(() => {
    async function loadStats() {
      try {
        const [telemetryDays, reviewItems] = await Promise.all([
          invoke<Day[]>('db_get_telemetry_days'),
          invoke<{ R: number }[]>('db_get_review_items'),
        ])
        setDays(telemetryDays)
        const mastered = reviewItems.filter((c) => c.R >= 3).length
        setMastery({
          mastered,
          total: reviewItems.length,
        })
        setKnownWords(mastered)
      } catch (e) {
        console.error('Failed to load stats:', e)
      }
    }
    loadStats()
  }, [])

  const streak = useMemo(() => computeStreak(days), [days])

  const sessionsToday = days.find((d) => d.date === todayKey())?.sessions ?? 0

  const totalSessions = days.reduce((sum, d) => sum + d.sessions, 0)

  const stats = [
    {
      icon: <FireOutlined />,
      label: 'Streak',
      value: streak,
      suffix: 'days',
      color: 'text-orange-500',
      bg: 'bg-orange-50',
    },
    {
      icon: <ThunderboltOutlined />,
      label: 'Today',
      value: sessionsToday,
      suffix: 'sessions',
      color: 'text-blue-500',
      bg: 'bg-blue-50',
    },
    {
      icon: <BookOutlined />,
      label: 'Mastery',
      value: mastery.mastered,
      suffix: `/ ${mastery.total}`,
      color: 'text-green-500',
      bg: 'bg-green-50',
    },
    {
      icon: <BulbOutlined />,
      label: 'Known words',
      value: knownWords,
      suffix: '',
      color: 'text-purple-500',
      bg: 'bg-purple-50',
    },
  ]

  // Last 7 days activity
  const last7 = useMemo(() => getLastNDays(days, 7), [days])

  const maxCount = Math.max(...last7.map((d) => d.count), 1)

  return (
    <div className="space-y-5">
      <h2 className="text-lg font-semibold">Your Stats</h2>

      {/* Stats grid */}
      <div className="grid grid-cols-2 gap-3">
        {stats.map((s) => (
          <div key={s.label} className="bg-white rounded-xl p-4 shadow-sm">
            <div className={`inline-flex items-center justify-center w-9 h-9 rounded-lg ${s.bg} ${s.color} text-lg mb-2`}>
              {s.icon}
            </div>
            <div className="flex items-baseline gap-1">
              <span className="text-2xl font-bold">{s.value}</span>
              {s.suffix && <span className="text-xs text-gray-400">{s.suffix}</span>}
            </div>
            <div className="text-xs text-gray-400 mt-0.5">{s.label}</div>
          </div>
        ))}
      </div>

      {/* Weekly activity */}
      <div className="bg-white rounded-xl p-4 shadow-sm">
        <h3 className="text-sm font-medium text-gray-600 mb-3">This Week</h3>
        <div className="flex items-end justify-between gap-1 h-28">
          {last7.map((d) => (
            <div key={d.day} className="flex flex-col items-center gap-1 flex-1">
              <div className="w-full flex items-end justify-center" style={{ height: 80 }}>
                <div
                  className={`w-full max-w-[28px] rounded-t-md transition-all ${
                    d.count > 0 ? 'bg-blue-500' : 'bg-gray-100'
                  }`}
                  style={{ height: `${Math.max((d.count / maxCount) * 100, 4)}%` }}
                />
              </div>
              <span className="text-[10px] text-gray-400">{d.day}</span>
            </div>
          ))}
        </div>
      </div>

      {/* Total sessions */}
      <div className="bg-white rounded-xl p-4 shadow-sm">
        <div className="flex items-center justify-between">
          <span className="text-sm text-gray-500">Total sessions</span>
          <span className="text-lg font-bold">{totalSessions}</span>
        </div>
      </div>
    </div>
  )
}
