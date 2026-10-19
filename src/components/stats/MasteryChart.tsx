import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
} from 'recharts'
import { t } from '../../lib/i18n'

interface Props {
  data: { date: string; label: string; mastered: number | null; total: number | null }[]
}

export default function MasteryChart({ data }: Props) {
  if (data.length === 0) {
    return (
      <div className="bg-white rounded-xl p-4 shadow-sm">
        <h3 className="text-sm font-medium text-gray-600 mb-3">{t('stats.mastery')}</h3>
        <p className="text-sm text-gray-400">{t('stats.noMastery')}</p>
      </div>
    )
  }

  return (
    <div className="bg-white rounded-xl p-4 shadow-sm">
      <div className="flex items-center justify-between mb-3">
        <h3 className="text-sm font-medium text-gray-600">{t('stats.mastery')}</h3>
        <div className="flex items-center gap-3 text-[10px]">
          <span className="flex items-center gap-1">
            <span className="w-2 h-2 rounded-full bg-green-500 inline-block" /> {t('stats.mastered')}
          </span>
          <span className="flex items-center gap-1">
            <span className="w-2 h-2 rounded-full bg-green-200 inline-block" /> {t('stats.totalCards')}
          </span>
        </div>
      </div>
      <ResponsiveContainer width="100%" height={200}>
        <LineChart data={data} margin={{ top: 5, right: 5, left: -30, bottom: 5 }}>
          <CartesianGrid strokeDasharray="3 3" stroke="#f0f0f0" />
          <XAxis dataKey="label" tick={{ fontSize: 10 }} stroke="#9ca3af" />
          <YAxis tick={{ fontSize: 10 }} stroke="#9ca3af" allowDecimals={false} />
          <Tooltip contentStyle={{ fontSize: 12, borderRadius: 8 }} />
          <Line type="monotone" dataKey="total" stroke="#bbf7d0" strokeWidth={2} dot={false} connectNulls={false} />
          <Line type="monotone" dataKey="mastered" stroke="#22c55e" strokeWidth={2} dot={{ r: 3, fill: '#dcfce7', stroke: '#22c55e', strokeWidth: 1.5 }} connectNulls={false} />
        </LineChart>
      </ResponsiveContainer>
    </div>
  )
}