import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
  ReferenceLine,
} from 'recharts'

interface Props {
  data: { date: string; label: string; avgScore: number; count: number }[]
  overallAvg: number | null
}

export default function ProficiencyChart({ data, overallAvg }: Props) {
  if (data.length === 0) {
    return (
      <div className="bg-white rounded-xl p-4 shadow-sm">
        <h3 className="text-sm font-medium text-gray-600 mb-3">Chat Proficiency</h3>
        <p className="text-sm text-gray-400">No evaluations yet</p>
      </div>
    )
  }

  return (
    <div className="bg-white rounded-xl p-4 shadow-sm">
      <div className="flex items-center justify-between mb-3">
        <h3 className="text-sm font-medium text-gray-600">Chat Proficiency</h3>
        {overallAvg !== null && (
          <span className="text-lg font-bold text-yellow-600">{overallAvg}</span>
        )}
      </div>
      <ResponsiveContainer width="100%" height={200}>
        <LineChart data={data} margin={{ top: 5, right: 5, left: -45, bottom: 5 }}>
          <CartesianGrid strokeDasharray="3 3" stroke="#f0f0f0" />
          <XAxis dataKey="label" tick={{ fontSize: 10 }} stroke="#9ca3af" />
          <YAxis tick={{ fontSize: 10 }} stroke="#9ca3af" domain={[0, 5]} />
          <Tooltip
            contentStyle={{ fontSize: 12, borderRadius: 8 }}
            formatter={(value, name) => [value, name === 'avgScore' ? 'Score' : name]}
          />
          {overallAvg !== null && (
            <ReferenceLine y={overallAvg} stroke="#fbbf24" strokeDasharray="5 5" strokeWidth={1} />
          )}
          <Line type="monotone" dataKey="avgScore" stroke="#eab308" strokeWidth={2} dot={{ r: 3, fill: '#fef08a', stroke: '#eab308', strokeWidth: 1.5 }} />
        </LineChart>
      </ResponsiveContainer>
    </div>
  )
}