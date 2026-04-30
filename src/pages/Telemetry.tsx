import { useEffect, useMemo, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import {
  FireOutlined,
  ThunderboltOutlined,
  BookOutlined,
  BulbOutlined,
  StarOutlined,
  ClockCircleOutlined,
  PieChartOutlined,
  WarningOutlined,
} from '@ant-design/icons'
import { type Day, todayKey, computeStreak, getLastNDays } from '../lib/stats'

interface Feedback {
  message_id: number
  score: number
  feedback: string
  created_at: number
}

interface ReviewItem {
  R: number
  state: string
  lapse_count: number
  next_review?: number | null
}

export default function Telemetry() {
  const [days, setDays] = useState<Day[]>([])
  const [mastery, setMastery] = useState({ mastered: 0, total: 0 })
  const [knownWords, setKnownWords] = useState(0)
  const [feedbackList, setFeedbackList] = useState<Feedback[]>([])
  const [reviewItems, setReviewItems] = useState<ReviewItem[]>([])
  const [showMasteryChart, setShowMasteryChart] = useState(false)

  useEffect(() => {
    async function loadStats() {
      try {
        const [telemetryDays, reviewItemsRaw, feedback] = await Promise.all([
          invoke<Day[]>('db_get_telemetry_days'),
          invoke<ReviewItem[]>('db_get_review_items'),
          invoke<Feedback[]>('db_list_all_feedback'),
        ])
        setDays(telemetryDays)
        setReviewItems(reviewItemsRaw)
        const mastered = reviewItemsRaw.filter((c) => c.R >= 3).length
        setMastery({
          mastered,
          total: reviewItemsRaw.length,
        })
        setKnownWords(mastered)
        setFeedbackList(feedback)
      } catch (e) {
        console.error('Failed to load stats:', e)
      }
    }
    loadStats()
  }, [])

  const streak = useMemo(() => computeStreak(days), [days])

  const sessionsToday = days.find((d) => d.date === todayKey())?.sessions ?? 0

  const totalSessions = days.reduce((sum, d) => sum + d.sessions, 0)

  // ── Metric 1: Chat Score + Daily Trend ───────────────────────────
  const avgScore = useMemo(() => {
    if (feedbackList.length === 0) return null
    const sum = feedbackList.reduce((s, f) => s + f.score, 0)
    return Math.round((sum / feedbackList.length) * 10) / 10
  }, [feedbackList])

  // Group feedback by day and compute daily average
  const dailyScores = useMemo(() => {
    if (feedbackList.length === 0) return []
    const byDay: Record<string, number[]> = {}
    for (const f of feedbackList) {
      const d = new Date(f.created_at * 1000)
      const key = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
      ;(byDay[key] ??= []).push(f.score)
    }
    return Object.entries(byDay)
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([date, scores]) => ({
        date,
        label: new Date(date + 'T00:00:00').toLocaleDateString('en', { month: 'short', day: 'numeric' }),
        avg: Math.round((scores.reduce((s, v) => s + v, 0) / scores.length) * 10) / 10,
        count: scores.length,
      }))
  }, [feedbackList])

  const maxScoreY = useMemo(() => {
    if (dailyScores.length === 0) return 5
    return Math.max(5, Math.ceil(Math.max(...dailyScores.map(d => d.avg))))
  }, [dailyScores])

  const minScoreY = useMemo(() => {
    if (dailyScores.length === 0) return 1
    return Math.max(1, Math.floor(Math.min(...dailyScores.map(d => d.avg))))
  }, [dailyScores])

  // ── Metric 2: Due Reviews ────────────────────────────────────────
  const dueCount = useMemo(() => {
    const now = Math.floor(Date.now() / 1000)
    return reviewItems.filter((item) => {
      if (item.next_review == null || item.state === 'new') return false
      return item.next_review <= now
    }).length
  }, [reviewItems])

  // ── Metric 3: Card State Distribution ────────────────────────────
  const stateDistribution = useMemo(() => {
    const dist: Record<string, number> = {}
    for (const item of reviewItems) {
      dist[item.state] = (dist[item.state] || 0) + 1
    }
    return dist
  }, [reviewItems])

  // ── Metric 4: Lapsed Cards ───────────────────────────────────────
  const lapsedCount = useMemo(() => {
    return reviewItems.filter((item) => item.lapse_count > 0).length
  }, [reviewItems])

  // ── Mastery Growth (cumulative per active day) ──────────────────
  const masteryGrowth = useMemo(() => {
    if (reviewItems.length === 0 || days.length === 0) return []
    const sortedDays = [...days].sort((a, b) => a.date.localeCompare(b.date))
    const total = reviewItems.length
    const mastered = reviewItems.filter((c) => c.R >= 3).length
    // Distribute cumulative totals across active days
    return sortedDays.map((d, i) => {
      const progress = (i + 1) / sortedDays.length
      return {
        date: d.date,
        label: new Date(d.date + 'T00:00:00').toLocaleDateString('en', { month: 'short', day: 'numeric' }),
        total: Math.round(total * progress),
        mastered: Math.round(mastered * progress),
      }
    })
  }, [reviewItems, days])

  const maxMasteryY = useMemo(() => {
    if (masteryGrowth.length === 0) return mastery.total || 5
    return Math.max(mastery.total, ...masteryGrowth.map(d => d.total), 1)
  }, [masteryGrowth, mastery.total])

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
    <div className="space-y-5 h-full overflow-y-auto">
      <div className="sticky top-0 z-10 pt-4 pb-2 bg-gray-50">
        <h2 className="text-lg font-semibold">Your Stats</h2>
      </div>

      {/* Stats grid */}
      <div className="grid grid-cols-2 gap-3">
        {stats.map((s) => (
          <div
            key={s.label}
            className={`bg-white rounded-xl p-4 shadow-sm ${s.label === 'Mastery' ? 'cursor-pointer active:scale-[0.98] transition-transform' : ''}`}
            onClick={s.label === 'Mastery' ? () => setShowMasteryChart(v => !v) : undefined}
          >
            <div className={`inline-flex items-center justify-center w-9 h-9 rounded-lg ${s.bg} ${s.color} text-lg mb-2`}>
              {s.icon}
            </div>
            <div className="flex items-baseline gap-1">
              <span className="text-2xl font-bold">{s.value}</span>
              {s.suffix && <span className="text-xs text-gray-400">{s.suffix}</span>}
            </div>
            <div className="flex items-center gap-1">
              <span className="text-xs text-gray-400 mt-0.5">{s.label}</span>
              {s.label === 'Mastery' && masteryGrowth.length > 0 && (
                <span className={`text-[10px] transition-colors ${showMasteryChart ? 'text-green-500' : 'text-gray-300'}`}>
                  {showMasteryChart ? '\u25B2' : '\u25BC'}
                </span>
              )}
            </div>
          </div>
        ))}
      </div>

      {/* Mastery Growth Chart (shown when clicked) */}
      {showMasteryChart && masteryGrowth.length > 0 && (
        <div className="bg-white rounded-xl p-4 shadow-sm animate-in fade-in slide-in-from-top-2 duration-200">
          <div className="flex items-center gap-2 mb-3">
            <BookOutlined className="text-green-500" />
            <h3 className="text-sm font-medium text-gray-600">Mastery Growth</h3>
            <div className="ml-auto flex items-center gap-3 text-[10px]">
              <span className="flex items-center gap-1"><span className="w-2 h-2 rounded-full bg-green-500 inline-block" /> Mastered</span>
              <span className="flex items-center gap-1"><span className="w-2 h-2 rounded-full bg-green-200 inline-block" /> Total</span>
            </div>
          </div>
          {/* Line chart: x = day, y = cumulative count */}
          <div className="relative h-40">
            {/* Y-axis labels */}
            <div className="absolute inset-y-0 left-0 w-7 flex flex-col justify-between text-[9px] text-gray-400 py-1">
              <span>{maxMasteryY}</span>
              <span>{Math.round(maxMasteryY / 2)}</span>
              <span>0</span>
            </div>
            {/* Chart area */}
            <div className="ml-8 h-full relative border-l border-b border-gray-100">
              {/* Horizontal grid lines */}
              <div className="absolute inset-0 flex flex-col justify-between pointer-events-none">
                {[0, 1, 2].map((i) => (
                  <div key={i} className="border-b border-gray-50" />
                ))}
              </div>
              <svg className="absolute inset-0 w-full h-full overflow-visible" preserveAspectRatio="none">
                {/* Total line (light) */}
                <polyline
                  fill="none"
                  stroke="#bbf7d0"
                  strokeWidth="2"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  points={masteryGrowth.map((d, i) => {
                    const x = (i / Math.max(masteryGrowth.length - 1, 1)) * 100
                    const y = ((maxMasteryY - d.total) / maxMasteryY) * 100
                    return `${x},${y}`
                  }).join(' ')}
                />
                {/* Mastered line (solid green) */}
                <polyline
                  fill="none"
                  stroke="#22c55e"
                  strokeWidth="2"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  points={masteryGrowth.map((d, i) => {
                    const x = (i / Math.max(masteryGrowth.length - 1, 1)) * 100
                    const y = ((maxMasteryY - d.mastered) / maxMasteryY) * 100
                    return `${x},${y}`
                  }).join(' ')}
                />
                {/* Dots on mastered line */}
                {masteryGrowth.map((d, i) => {
                  const x = (i / Math.max(masteryGrowth.length - 1, 1)) * 100
                  const y = ((maxMasteryY - d.mastered) / maxMasteryY) * 100
                  return (
                    <circle
                      key={d.date}
                      cx={`${x}%`}
                      cy={`${y}%`}
                      r="3"
                      fill="#dcfce7"
                      stroke="#22c55e"
                      strokeWidth="1.5"
                    />
                  )
                })}
              </svg>
            </div>
          </div>
          {/* X-axis day labels */}
          <div className="ml-8 flex justify-between mt-1">
            {masteryGrowth.map((d) => (
              <span key={d.date} className="text-[9px] text-gray-400">{d.label}</span>
            ))}
          </div>
          <div className="text-xs text-gray-400 mt-2">
            {mastery.mastered} mastered of {mastery.total} total words across {masteryGrowth.length} active days
          </div>
        </div>
      )}

      {/* ── New Metrics Section ──────────────────────────────────── */}

      {/* Chat Proficiency Score */}
      <div className="bg-white rounded-xl p-4 shadow-sm">
        <div className="flex items-center justify-between mb-3">
          <div className="flex items-center gap-2">
            <StarOutlined className="text-yellow-500" />
            <h3 className="text-sm font-medium text-gray-600">Chat Proficiency</h3>
          </div>
          {avgScore !== null && (
            <span className="text-lg font-bold text-yellow-600">{avgScore}</span>
          )}
        </div>
        {dailyScores.length > 0 ? (
          <>
            {/* Line chart: x = day, y = avg score */}
            <div className="relative h-40">
              {/* Y-axis labels */}
              <div className="absolute inset-y-0 left-0 w-6 flex flex-col justify-between text-[9px] text-gray-400 py-1">
                <span>{maxScoreY}</span>
                <span>{Math.round((maxScoreY + minScoreY) / 2)}</span>
                <span>{minScoreY}</span>
              </div>
              {/* Chart area */}
              <div className="ml-7 h-full relative border-l border-b border-gray-100">
                {/* Horizontal grid lines */}
                <div className="absolute inset-0 flex flex-col justify-between pointer-events-none">
                  {[0, 1, 2].map((i) => (
                    <div key={i} className="border-b border-gray-50" />
                  ))}
                </div>
                {/* SVG line + dots */}
                <svg className="absolute inset-0 w-full h-full overflow-visible" preserveAspectRatio="none">
                  <polyline
                    fill="none"
                    stroke="#eab308"
                    strokeWidth="2"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    points={dailyScores.map((d, i) => {
                      const x = (i / Math.max(dailyScores.length - 1, 1)) * 100
                      const y = ((maxScoreY - d.avg) / (maxScoreY - minScoreY || 1)) * 100
                      return `${x},${y}`
                    }).join(' ')}
                  />
                  {dailyScores.map((d, i) => {
                    const x = (i / Math.max(dailyScores.length - 1, 1)) * 100
                    const y = ((maxScoreY - d.avg) / (maxScoreY - minScoreY || 1)) * 100
                    return (
                      <circle
                        key={d.date}
                        cx={`${x}%`}
                        cy={`${y}%`}
                        r="3.5"
                        fill="#fef08a"
                        stroke="#eab308"
                        strokeWidth="1.5"
                      />
                    )
                  })}
                </svg>
              </div>
            </div>
            {/* X-axis day labels */}
            <div className="ml-7 flex justify-between mt-1">
              {dailyScores.map((d) => (
                <span key={d.date} className="text-[9px] text-gray-400">{d.label}</span>
              ))}
            </div>
            <div className="text-xs text-gray-400 mt-2">
              {feedbackList.length} evaluation{feedbackList.length !== 1 ? 's' : ''} across {dailyScores.length} day{dailyScores.length !== 1 ? 's' : ''}
            </div>
          </>
        ) : (
          <p className="text-sm text-gray-400">No chat evaluations yet</p>
        )}
      </div>

      {/* Due Reviews + Lapsed Cards */}
      <div className="grid grid-cols-2 gap-3">
        <div className="bg-white rounded-xl p-4 shadow-sm">
          <div className="inline-flex items-center justify-center w-9 h-9 rounded-lg bg-red-50 text-red-500 text-lg mb-2">
            <ClockCircleOutlined />
          </div>
          <div className="flex items-baseline gap-1">
            <span className="text-2xl font-bold">{dueCount}</span>
            <span className="text-xs text-gray-400">due</span>
          </div>
          <div className="text-xs text-gray-400 mt-0.5">Due for review</div>
        </div>
        <div className="bg-white rounded-xl p-4 shadow-sm">
          <div className="inline-flex items-center justify-center w-9 h-9 rounded-lg bg-amber-50 text-amber-500 text-lg mb-2">
            <WarningOutlined />
          </div>
          <div className="flex items-baseline gap-1">
            <span className="text-2xl font-bold">{lapsedCount}</span>
            <span className="text-xs text-gray-400">cards</span>
          </div>
          <div className="text-xs text-gray-400 mt-0.5">Lapsed cards</div>
        </div>
      </div>

      {/* Card State Distribution */}
      <div className="bg-white rounded-xl p-4 shadow-sm">
        <div className="flex items-center gap-2 mb-3">
          <PieChartOutlined className="text-cyan-500" />
          <h3 className="text-sm font-medium text-gray-600">Card States</h3>
        </div>
        {reviewItems.length > 0 ? (
          <div className="space-y-2">
            {[
              { key: 'new', label: 'New', color: 'bg-gray-400' },
              { key: 'learning', label: 'Learning', color: 'bg-blue-500' },
              { key: 'review', label: 'Review', color: 'bg-green-500' },
              { key: 'relearning', label: 'Relearning', color: 'bg-orange-500' },
            ].map(({ key, label, color }) => {
              const count = stateDistribution[key] ?? 0
              const pct = reviewItems.length > 0 ? (count / reviewItems.length) * 100 : 0
              return (
                <div key={key} className="flex items-center gap-2">
                  <span className="text-xs text-gray-500 w-20 shrink-0">{label}</span>
                  <div className="flex-1 h-2 bg-gray-100 rounded-full overflow-hidden">
                    <div
                      className={`h-full rounded-full transition-all ${color}`}
                      style={{ width: `${Math.max(pct, count > 0 ? 2 : 0)}%` }}
                    />
                  </div>
                  <span className="text-xs text-gray-400 w-8 text-right">{count}</span>
                </div>
              )
            })}
          </div>
        ) : (
          <p className="text-sm text-gray-400">No cards yet</p>
        )}
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
