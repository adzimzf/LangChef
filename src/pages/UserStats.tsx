import { useEffect, useMemo, useState, lazy, Suspense } from 'react'
import { invoke } from '@tauri-apps/api/core'
import {
  type ActivityRow,
  type ActiveTimeRow,
  type TimeRange,
  todayKey,
  localDateKey,
  computeStreakFromActivity,
  getDateRange,
  formatActiveTime,
  computeTrend,
} from '../lib/stats'
import TimeRangeFilter from '../components/stats/TimeRangeFilter'
import StreakCard from '../components/stats/StreakCard'
import ActiveTimeCard from '../components/stats/ActiveTimeCard'
import StatsSkeleton from '../components/stats/StatsSkeleton'
import { t, locale } from '../lib/i18n'

const ActivityChart = lazy(() => import('../components/stats/ActivityChart'))
const MasteryChart = lazy(() => import('../components/stats/MasteryChart'))
const ProficiencyChart = lazy(() => import('../components/stats/ProficiencyChart'))
const ReviewBreakdown = lazy(() => import('../components/stats/ReviewBreakdown'))

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

const STORAGE_KEY = 'stats_time_range'

export default function UserStats() {
  const saved = (typeof window !== 'undefined' ? localStorage.getItem(STORAGE_KEY) : null) as TimeRange | null
  const [timeRange, setTimeRange] = useState<TimeRange>(saved || 'weekly')
  const [activity, setActivity] = useState<ActivityRow[]>([])
  const [activeTime, setActiveTime] = useState<ActiveTimeRow[]>([])
  const [reviewItems, setReviewItems] = useState<ReviewItem[]>([])
  const [feedbackList, setFeedbackList] = useState<Feedback[]>([])
  const [masteryHistory, setMasteryHistory] = useState<{ date: string; mastered: number }[]>([])
  const [cardsAddedHistory, setCardsAddedHistory] = useState<{ date: string; total: number }[]>([])
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    localStorage.setItem(STORAGE_KEY, timeRange)
  }, [timeRange])

  useEffect(() => {
    const { dateFrom, dateTo } = getDateRange(timeRange)
    async function load() {
      setLoading(true)
      try {
        const [act, time, items, fb, mh, cah] = await Promise.all([
          invoke<ActivityRow[]>('db_get_user_activity', { dateFrom, dateTo }),
          invoke<ActiveTimeRow[]>('db_get_active_time', { dateFrom, dateTo }),
          invoke<ReviewItem[]>('db_get_review_items'),
          invoke<Feedback[]>('db_list_all_feedback'),
          invoke<{ date: string; mastered: number }[]>('db_get_mastery_history', { dateFrom, dateTo }),
          invoke<{ date: string; total: number }[]>('db_get_cards_added_history', { dateFrom, dateTo }),
        ])
        setActivity(act)
        setActiveTime(time)
        setReviewItems(items)
        setFeedbackList(fb)
        setMasteryHistory(mh)
        setCardsAddedHistory(cah)
      } catch (e) {
        console.error('Failed to load stats:', e)
      }
      setLoading(false)
    }
    load()
  }, [timeRange])

  // ── Streak ────────────────────────────────────────────────────────
  const streak = useMemo(() => computeStreakFromActivity(activity), [activity])

  // Load all-time activity for longest streak
  const [allTimeActivity, setAllTimeActivity] = useState<ActivityRow[]>([])
  useEffect(() => {
    invoke<ActivityRow[]>('db_get_user_activity', { dateFrom: '2000-01-01', dateTo: todayKey() })
      .then(setAllTimeActivity)
      .catch(() => {})
  }, [])
  const longestStreak = useMemo(() => computeStreakFromActivity(allTimeActivity), [allTimeActivity])

  // Recent 7-day activity for heatmap
  const recentActivity = useMemo(() => {
    const result: { date: string; hasActivity: boolean }[] = []
    const activeDates = new Set(activity.map(a => a.date))
    for (let i = 6; i >= 0; i--) {
      const d = new Date()
      d.setDate(d.getDate() - i)
      const key = localDateKey(d)
      result.push({ date: key, hasActivity: activeDates.has(key) })
    }
    return result
  }, [activity])

  // ── Active Time ───────────────────────────────────────────────────
  const todaySeconds = useMemo(() => {
    const today = todayKey()
    return activeTime.find(t => t.date === today)?.total_seconds ?? 0
  }, [activeTime])

  const avgSeconds = useMemo(() => {
    if (activeTime.length === 0) return 0
    const total = activeTime.reduce((s, t) => s + t.total_seconds, 0)
    return Math.round(total / activeTime.length)
  }, [activeTime])

  // Previous period for trend
  const [prevActiveTime, setPrevActiveTime] = useState<ActiveTimeRow[]>([])
  useEffect(() => {
    const { dateFrom, dateTo } = getDateRange(timeRange)
    const rangeStart = new Date(dateFrom + 'T00:00:00')
    const rangeEnd = new Date(dateTo + 'T00:00:00')
    const rangeDays = (rangeEnd.getTime() - rangeStart.getTime()) / (24 * 60 * 60 * 1000)
    const prevEnd = new Date(rangeStart)
    prevEnd.setDate(prevEnd.getDate() - 1)
    const prevStart = new Date(prevEnd)
    prevStart.setDate(prevStart.getDate() - rangeDays)
    invoke<ActiveTimeRow[]>('db_get_active_time', {
      dateFrom: localDateKey(prevStart),
      dateTo: localDateKey(prevEnd),
    }).then(setPrevActiveTime).catch(() => {})
  }, [timeRange])

  const prevAvgSeconds = useMemo(() => {
    if (prevActiveTime.length === 0) return 0
    const total = prevActiveTime.reduce((s, t) => s + t.total_seconds, 0)
    return Math.round(total / prevActiveTime.length)
  }, [prevActiveTime])

  const timeTrend = useMemo(() => computeTrend(avgSeconds, prevAvgSeconds), [avgSeconds, prevAvgSeconds])

  // ── Activity Chart Data ───────────────────────────────────────────
  const activityChartData = useMemo(() => {
    const { dateFrom, dateTo } = getDateRange(timeRange)
    const result: { date: string; label: string; chat: number; review: number; movingAvg: number }[] = []
    const start = new Date(dateFrom + 'T00:00:00')
    const end = new Date(dateTo + 'T00:00:00')
    const byDate: Record<string, { chat: number; review: number }> = {}
    for (const a of activity) {
      if (!byDate[a.date]) byDate[a.date] = { chat: 0, review: 0 }
      if (a.activity_type === 'chat') byDate[a.date].chat += a.count
      else if (a.activity_type === 'review') byDate[a.date].review += a.count
    }
    const allDates: string[] = []
    for (let d = new Date(start); d <= end; d.setDate(d.getDate() + 1)) {
      allDates.push(localDateKey(d))
    }
    // Compute 7-day moving average
    const rawValues = allDates.map(date => {
      const v = byDate[date] || { chat: 0, review: 0 }
      return v.chat + v.review
    })
    const movingAvgs = rawValues.map((_, i) => {
      const window = rawValues.slice(Math.max(0, i - 3), Math.min(rawValues.length, i + 4))
      const sum = window.reduce((s, v) => s + v, 0)
      return Math.round((sum / window.length) * 10) / 10
    })
    return allDates.map((date, i) => {
      const v = byDate[date] || { chat: 0, review: 0 }
      return {
        date,
        label: new Date(date + 'T00:00:00').toLocaleDateString(locale, { month: 'short', day: 'numeric' }),
        chat: v.chat,
        review: v.review,
        movingAvg: movingAvgs[i],
      }
    })
  }, [activity, timeRange])

  // ── Total Sessions ────────────────────────────────────────────────
  const totalSessions = useMemo(() => activity.reduce((s, a) => s + a.count, 0), [activity])
  const chatSessions = useMemo(() => activity.filter(a => a.activity_type === 'chat').reduce((s, a) => s + a.count, 0), [activity])
  const reviewSessions = useMemo(() => activity.filter(a => a.activity_type === 'review').reduce((s, a) => s + a.count, 0), [activity])

  // ── Mastery ───────────────────────────────────────────────────────
  const mastery = useMemo(() => {
    const mastered = reviewItems.filter(c => c.R >= 3).length
    return { mastered, total: reviewItems.length }
  }, [reviewItems])

  const masteryChartData = useMemo(() => {
    if (activityChartData.length === 0) return []
    const masteryByDate = Object.fromEntries(masteryHistory.map(m => [m.date, m.mastered]))
    const totalByDate = Object.fromEntries(cardsAddedHistory.map(c => [c.date, c.total]))
    return activityChartData.map(d => ({
      date: d.date,
      label: d.label,
      total: totalByDate[d.date] ?? null,
      mastered: masteryByDate[d.date] ?? null,
    }))
  }, [reviewItems, activityChartData, masteryHistory, cardsAddedHistory])

  // ── Chat Proficiency ──────────────────────────────────────────────
  const avgScore = useMemo(() => {
    if (feedbackList.length === 0) return null
    const sum = feedbackList.reduce((s, f) => s + f.score, 0)
    return Math.round((sum / feedbackList.length) * 10) / 10
  }, [feedbackList])

  const proficiencyChartData = useMemo(() => {
    if (feedbackList.length === 0) return []
    const { dateFrom, dateTo } = getDateRange(timeRange)
    const byDay: Record<string, number[]> = {}
    for (const f of feedbackList) {
      const d = new Date(f.created_at * 1000)
      const key = localDateKey(d)
      if (key < dateFrom || key > dateTo) continue
      ;(byDay[key] ??= []).push(f.score)
    }
    return Object.entries(byDay)
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([date, scores]) => ({
        date,
        label: new Date(date + 'T00:00:00').toLocaleDateString(locale, { month: 'short', day: 'numeric' }),
        avgScore: Math.round((scores.reduce((s, v) => s + v, 0) / scores.length) * 10) / 10,
        count: scores.length,
      }))
  }, [feedbackList, timeRange])

  // ── Review Breakdown ──────────────────────────────────────────────
  const stateDistribution = useMemo(() => {
    const dist: Record<string, number> = {}
    for (const item of reviewItems) {
      dist[item.state] = (dist[item.state] || 0) + 1
    }
    return [
      { state: 'new', count: dist['new'] ?? 0, color: '#9ca3af' },
      { state: 'learning', count: dist['learning'] ?? 0, color: '#3b82f6' },
      { state: 'review', count: dist['review'] ?? 0, color: '#22c55e' },
      { state: 'relearning', count: dist['relearning'] ?? 0, color: '#f97316' },
    ]
  }, [reviewItems])

  const dueCount = useMemo(() => {
    const now = Math.floor(Date.now() / 1000)
    return reviewItems.filter(item => {
      if (item.next_review == null || item.state === 'new') return false
      return item.next_review <= now
    }).length
  }, [reviewItems])

  const lapsedCount = useMemo(() => {
    return reviewItems.filter(item => item.lapse_count > 0).length
  }, [reviewItems])

  // ── Empty State ───────────────────────────────────────────────────
  const isEmpty = !loading && activity.length === 0 && activeTime.length === 0 && reviewItems.length === 0

  return (
    <div className="space-y-5 h-full overflow-y-auto">
      <div className="sticky top-0 z-10 pt-4 pb-2 bg-gray-50 space-y-3">
        <h2 className="text-lg font-semibold">{t('home.stats')}</h2>
        <TimeRangeFilter value={timeRange} onChange={setTimeRange} />
      </div>

      {loading ? (
        <StatsSkeleton />
      ) : isEmpty ? (
        <div className="flex flex-col items-center justify-center py-20 text-gray-400">
          <div className="text-4xl mb-3">&#x1F4CA;</div>
          <p className="text-sm">{t('stats.empty')}</p>
          <p className="text-xs mt-1">{t('stats.emptyHint')}</p>
        </div>
      ) : (
        <>
          {/* Hero Section */}
          <div className="grid grid-cols-2 gap-3">
            <StreakCard streak={streak} longestStreak={longestStreak} recentActivity={recentActivity} />
            <ActiveTimeCard todaySeconds={todaySeconds} avgSeconds={avgSeconds} trend={timeTrend} />
          </div>

          {/* Activity Section */}
          <Suspense fallback={<StatsSkeleton />}>
            <ActivityChart data={activityChartData} />
          </Suspense>

          <div className="bg-white rounded-xl p-4 shadow-sm">
            <div className="flex items-center justify-between">
              <span className="text-sm text-gray-500">{t('stats.totalSessions')}</span>
              <span className="text-lg font-bold">{totalSessions}</span>
            </div>
            <div className="flex items-center justify-between mt-1">
              <span className="text-xs text-gray-400">{t('stats.sessionSplit', { chat: chatSessions, review: reviewSessions })}</span>
            </div>
          </div>

          {/* Progress Section */}
          <div className="grid grid-cols-1 gap-3">
            <Suspense fallback={<StatsSkeleton />}>
              <MasteryChart data={masteryChartData} />
            </Suspense>
            <Suspense fallback={<StatsSkeleton />}>
              <ProficiencyChart data={proficiencyChartData} overallAvg={avgScore} />
            </Suspense>
          </div>

          {/* Review Section */}
          <Suspense fallback={<StatsSkeleton />}>
            <ReviewBreakdown distribution={stateDistribution} dueCount={dueCount} lapsedCount={lapsedCount} />
          </Suspense>
        </>
      )}
    </div>
  )
}