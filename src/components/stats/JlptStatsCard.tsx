import { useNavigate } from 'react-router-dom'
import { LineChart, Line, XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer } from 'recharts'
import { RightOutlined } from '@ant-design/icons'
import type { JlptSection, JlptStats } from '../../lib/types'
import { SECTION_JA } from '../../lib/jlpt'
import { t, formatDate } from '../../lib/i18n'

/** One color per section, the same in the tiles and the chart. */
const SECTION_COLOR: Record<JlptSection, string> = {
  vocab: '#3b82f6',
  grammar: '#6366f1',
  reading: '#14b8a6',
  listening: '#e11d48',
}

export default function JlptStatsCard({ stats }: { stats: JlptStats }) {
  const navigate = useNavigate()
  const chart = stats.days.map((d) => ({ ...d, label: formatDate(`${d.date}T00:00:00`, { day: 'numeric', month: 'short' }) }))
  const sections = stats.sections.map((s) => s.section)

  return (
    <div className="bg-white rounded-xl p-4 shadow-sm flex flex-col gap-4">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-medium text-gray-600">{t('stats.jlptTitle')}</h3>
        <button onClick={() => navigate('/jlpt')} className="text-xs text-blue-500 flex items-center gap-1">
          {t('stats.jlptOpen')} <RightOutlined className="text-[10px]" />
        </button>
      </div>

      {stats.sessions === 0 ? (
        <p className="text-sm text-gray-400">{t('stats.jlptEmpty')}</p>
      ) : (
        <>
          <div className="grid grid-cols-2 gap-2">
            <div className="bg-gray-50 rounded-xl p-3">
              <div className="text-lg font-bold tabular-nums">{stats.sessions}</div>
              <div className="text-[11px] text-gray-500">{t('stats.jlptSessions', { count: stats.sessions })}</div>
            </div>
            <div className="bg-gray-50 rounded-xl p-3">
              <div className="text-lg font-bold tabular-nums">{stats.in_time ?? '—'}%</div>
              <div className="text-[11px] text-gray-500">{t('stats.jlptInTime')}</div>
            </div>
          </div>

          <div className="flex flex-col gap-2.5">
            {stats.sections.map((s) => (
              <div key={s.section} className="flex flex-col gap-1">
                <div className="flex items-center justify-between text-sm">
                  <span>
                    <span className="inline-block w-2 h-2 rounded-full mr-1.5 align-middle" style={{ background: SECTION_COLOR[s.section] }} />
                    {SECTION_JA[s.section]} <span className="text-gray-400 text-xs">{t(`jlpt.section.${s.section}`)}</span>
                  </span>
                  <span className="tabular-nums">
                    <b>{s.accuracy !== null ? `${s.accuracy}%` : '—'}</b>
                    <span className="text-[11px] text-gray-400 ml-1.5">{t('stats.jlptSessions', { count: s.sessions })}</span>
                  </span>
                </div>
                <div className="h-1.5 rounded-full bg-gray-100 overflow-hidden">
                  <div className="h-full rounded-full anim-grow" style={{ width: `${s.accuracy ?? 0}%`, background: SECTION_COLOR[s.section] }} />
                </div>
              </div>
            ))}
          </div>

          {chart.length > 1 && (
            <div>
              <div className="text-[11px] text-gray-400 mb-1">{t('stats.jlptTrend')}</div>
              <ResponsiveContainer width="100%" height={180}>
                <LineChart data={chart} margin={{ top: 5, right: 5, left: -30, bottom: 5 }}>
                  <CartesianGrid strokeDasharray="3 3" stroke="#f0f0f0" />
                  <XAxis dataKey="label" tick={{ fontSize: 10 }} stroke="#9ca3af" />
                  <YAxis tick={{ fontSize: 10 }} stroke="#9ca3af" domain={[0, 100]} unit="%" />
                  <Tooltip
                    contentStyle={{ fontSize: 12, borderRadius: 8 }}
                    formatter={(value, name) => [`${value}%`, SECTION_JA[String(name)] ?? name]}
                  />
                  {sections.map((s) => (
                    <Line key={s} type="monotone" dataKey={s} stroke={SECTION_COLOR[s]} strokeWidth={2} connectNulls dot={{ r: 3, fill: '#fff', stroke: SECTION_COLOR[s], strokeWidth: 1.5 }} />
                  ))}
                </LineChart>
              </ResponsiveContainer>
            </div>
          )}
        </>
      )}

      {stats.estimates.length > 0 && (
        <div className="flex flex-col gap-1.5 border-t border-gray-100 pt-3">
          <div className="text-[11px] text-gray-400">{t('stats.jlptEstimate')}</div>
          {stats.estimates.map(({ level, estimate }) => (
            <div key={level} className="flex items-center justify-between text-sm">
              <span className="font-bold">{level}</span>
              <span className="flex items-center gap-2">
                <span className="tabular-nums"><b>{estimate.total}</b><span className="text-gray-400 text-xs">{t('jlpt.passMark', { max: estimate.max, pass: estimate.pass_mark })}</span></span>
                <span className={`text-[11px] px-2 py-0.5 rounded-full font-medium ${estimate.passed ? 'bg-green-50 text-green-600' : 'bg-red-50 text-red-500'}`}>
                  {estimate.passed ? t('jlpt.onTrack') : t('jlpt.below')}
                </span>
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}
