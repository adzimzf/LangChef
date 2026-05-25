import { PieChartOutlined, ClockCircleOutlined, WarningOutlined } from '@ant-design/icons'
import { PieChart, Pie, Cell, ResponsiveContainer, Tooltip } from 'recharts'

interface Props {
  distribution: { state: string; count: number; color: string }[]
  dueCount: number
  lapsedCount: number
}

const STATE_LABELS: Record<string, string> = {
  new: 'New',
  learning: 'Learning',
  review: 'Review',
  relearning: 'Relearning',
}

export default function ReviewBreakdown({ distribution, dueCount, lapsedCount }: Props) {
  const total = distribution.reduce((s, d) => s + d.count, 0)

  if (total === 0) {
    return (
      <div className="bg-white rounded-xl p-4 shadow-sm">
        <h3 className="text-sm font-medium text-gray-600 mb-3">Review Breakdown</h3>
        <p className="text-sm text-gray-400">No cards yet</p>
      </div>
    )
  }

  return (
    <div className="bg-white rounded-xl p-4 shadow-sm">
      <div className="flex items-center gap-2 mb-3">
        <PieChartOutlined className="text-cyan-500" />
        <h3 className="text-sm font-medium text-gray-600">Review Breakdown</h3>
      </div>
      <div className="flex items-center gap-4">
        <div className="w-28 h-28 shrink-0">
          <ResponsiveContainer width="100%" height="100%">
            <PieChart>
              <Pie
                data={distribution.filter(d => d.count > 0)}
                cx="50%"
                cy="50%"
                innerRadius={25}
                outerRadius={45}
                paddingAngle={2}
                dataKey="count"
                nameKey="state"
              >
                {distribution.filter(d => d.count > 0).map((d) => (
                  <Cell key={d.state} fill={d.color} />
                ))}
              </Pie>
              <Tooltip
                contentStyle={{ fontSize: 12, borderRadius: 8 }}
                formatter={(value: number, _name: string, props: { payload: { state: string } }) => [
                  value,
                  STATE_LABELS[props.payload.state] || props.payload.state,
                ]}
              />
            </PieChart>
          </ResponsiveContainer>
        </div>
        <div className="flex-1 space-y-1.5">
          {distribution.map((d) => (
            <div key={d.state} className="flex items-center gap-2">
              <span className="w-2.5 h-2.5 rounded-full shrink-0" style={{ backgroundColor: d.color }} />
              <span className="text-xs text-gray-500">{STATE_LABELS[d.state] || d.state}</span>
              <span className="text-xs text-gray-400 ml-auto">{d.count}</span>
            </div>
          ))}
        </div>
      </div>
      <div className="grid grid-cols-2 gap-3 mt-4">
        <div className="flex items-center gap-2">
          <ClockCircleOutlined className="text-red-500 text-sm" />
          <span className="text-xs text-gray-500">{dueCount} due</span>
        </div>
        <div className="flex items-center gap-2">
          <WarningOutlined className="text-amber-500 text-sm" />
          <span className="text-xs text-gray-500">{lapsedCount} lapsed</span>
        </div>
      </div>
    </div>
  )
}