import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { invoke } from '@tauri-apps/api/core'
import {
  MessageOutlined,
  BookOutlined,
  FireOutlined,
  ThunderboltOutlined,
  RightOutlined,
  UnorderedListOutlined,
  ReadOutlined,
  FileTextOutlined,
  FormOutlined,
} from '@ant-design/icons'
import { type ActivityRow, todayKey, computeStreakFromActivity } from '../lib/stats'
import { t } from '../lib/i18n'
import { SECTION_JA, practiceLevel } from '../lib/jlpt'
import type { JlptOverview } from '../lib/types'

/** The JLPT row says what comes next in the rotation, or that today's session is done. */
function jlptDesc(o: JlptOverview | null): string {
  if (!o) return t('home.jlptDesc')
  const next = o.plans.find((p) => p.section === o.next_section)
  if (o.done_today) return t('home.jlptDone', { section: SECTION_JA[o.next_section] })
  if (!next) return t('home.jlptDesc')
  const time = `${Math.floor(next.time_limit / 60)}:${String(next.time_limit % 60).padStart(2, '0')}`
  return t('home.jlptNext', { section: SECTION_JA[next.section], count: next.questions, time })
}

export default function Home() {
  const navigate = useNavigate()

  const [stats, setStats] = useState({
    streak: 0,
    mastered: 0,
    total: 0,
    sessionsToday: 0,
    knownWords: 0,
  })

  const [jlpt, setJlpt] = useState<JlptOverview | null>(null)

  useEffect(() => {
    invoke<JlptOverview>('db_jlpt_overview', { level: practiceLevel() }).then(setJlpt).catch(() => {})
  }, [])

  useEffect(() => {
    async function loadStats() {
      try {
        const [activityRows, reviewItems] = await Promise.all([
          invoke<ActivityRow[]>('db_get_user_activity', { dateFrom: '2000-01-01', dateTo: todayKey() }),
          invoke<{ R: number; state: string }[]>('db_get_review_items'),
        ])
        const streak = computeStreakFromActivity(activityRows)
        const mastered = reviewItems.filter((c) => c.state === 'review').length
        const today = todayKey()
        const sessionsToday = activityRows
          .filter(a => a.date === today)
          .reduce((sum, a) => sum + a.count, 0)

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
      label: t('home.chat'),
      desc: t('home.chatDesc'),
      color: 'bg-blue-500',
      onClick: () => navigate('/chats'),
    },
    {
      key: 'review',
      icon: <BookOutlined className="text-xl" />,
      label: t('home.review'),
      desc: t('home.reviewDesc', { mastered: stats.mastered, total: stats.total }),
      color: 'bg-green-500',
      onClick: () => navigate('/review'),
    },
    {
      key: 'jlpt',
      icon: <FormOutlined className="text-xl" />,
      label: t('home.jlpt'),
      desc: jlptDesc(jlpt),
      color: 'bg-rose-500',
      isNew: true,
      onClick: () => navigate('/jlpt'),
    },
    {
      key: 'words',
      icon: <UnorderedListOutlined className="text-xl" />,
      label: t('home.words'),
      desc: t('home.wordsDesc', { count: stats.total }),
      color: 'bg-amber-500',
      onClick: () => navigate('/words'),
    },
    {
      key: 'reading',
      icon: <FileTextOutlined className="text-xl" />,
      label: t('home.reading'),
      desc: t('home.readingDesc'),
      color: 'bg-teal-500',
      onClick: () => navigate('/reading'),
    },
    {
      key: 'grammar',
      icon: <ReadOutlined className="text-xl" />,
      label: t('home.grammar'),
      desc: t('home.grammarDesc'),
      color: 'bg-indigo-500',
      onClick: () => navigate('/grammar'),
    },
    {
      key: 'telemetry',
      icon: <ThunderboltOutlined className="text-xl" />,
      label: t('home.stats'),
      desc: t('home.statsDesc', { count: stats.sessionsToday }),
      color: 'bg-purple-500',
      onClick: () => navigate('/stats'),
    },
  ]

  return (
    <div className="space-y-5 h-full overflow-y-auto">
      {/* Sticky page header */}
      <div className="sticky top-0 z-10 pt-4 pb-2 bg-gray-50">
        <h2 className="text-lg font-semibold">LangChef</h2>
      </div>

      {/* Hero section */}
      <div className="bg-gradient-to-br from-blue-500 to-blue-600 rounded-2xl p-5 text-white">
        <h2 className="text-xl font-bold mb-1">LangChef</h2>
        <p className="text-blue-100 text-sm mb-4">{t('home.tagline')}</p>
        <div className="flex gap-4">
          <div className="flex items-center gap-2">
            <FireOutlined className="text-orange-300 text-lg" />
            <span className="font-semibold text-lg">{stats.streak}</span>
            <span className="text-blue-200 text-sm">{t('home.streak', { count: stats.streak })}</span>
          </div>
          <div className="flex items-center gap-2">
            <BookOutlined className="text-green-300 text-lg" />
            <span className="font-semibold text-lg">{stats.knownWords}</span>
            <span className="text-blue-200 text-sm">{t('home.knownWords', { count: stats.knownWords })}</span>
          </div>
        </div>
      </div>

      {/* Quick actions */}
      <div className="space-y-2">
        <h3 className="text-sm font-medium text-gray-500 uppercase tracking-wide px-1">{t('home.quickActions')}</h3>
        {actions.map((a) => (
          <button
            key={a.key}
            onClick={a.onClick}
            className="w-full flex items-center gap-3 bg-white rounded-xl p-4 shadow-sm hover:shadow active:bg-gray-50 transition-all text-left"
          >
            <div className={`w-11 h-11 rounded-xl ${a.color} flex items-center justify-center text-white shrink-0 ${a.isNew ? 'jlpt-icon-shine' : ''}`}>
              {a.icon}
            </div>
            <div className="flex-1 min-w-0">
              <div className="font-medium text-[15px]">
                {a.label}
                {a.isNew && <span className="jlpt-new ml-1.5 align-[2px] text-[9.5px] font-bold text-white bg-rose-500 rounded px-1">{t('home.new')}</span>}
              </div>
              <div className="text-xs text-gray-400">{a.desc}</div>
            </div>
            <RightOutlined className="text-gray-300" />
          </button>
        ))}
      </div>

      {/* Tip card */}
      <div className="bg-amber-50 border border-amber-100 rounded-xl p-4">
        <p className="text-sm text-amber-800">
          <strong>{t('home.tipLabel')}</strong> {t('home.tip')}
        </p>
      </div>
    </div>
  )
}
