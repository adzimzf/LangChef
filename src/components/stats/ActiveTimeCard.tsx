import { ClockCircleOutlined, CaretUpOutlined, CaretDownOutlined, MinusOutlined } from '@ant-design/icons'
import { formatActiveTime } from '../../lib/stats'

interface Props {
  todaySeconds: number
  avgSeconds: number
  trend: 'up' | 'down' | 'flat'
}

export default function ActiveTimeCard({ todaySeconds, avgSeconds, trend }: Props) {
  const TrendIcon = trend === 'up' ? CaretUpOutlined : trend === 'down' ? CaretDownOutlined : MinusOutlined
  const trendColor = trend === 'up' ? 'text-green-500' : trend === 'down' ? 'text-red-500' : 'text-gray-400'

  return (
    <div className="bg-white rounded-xl p-4 shadow-sm">
      <div className="inline-flex items-center justify-center w-9 h-9 rounded-lg bg-blue-50 text-blue-500 text-lg mb-2">
        <ClockCircleOutlined />
      </div>
      <div className="flex items-baseline gap-1">
        <span className="text-3xl font-bold">{formatActiveTime(todaySeconds)}</span>
        <span className="text-xs text-gray-400">today</span>
      </div>
      <div className="flex items-center gap-1 mt-1">
        <span className="text-xs text-gray-400">
          Avg: {formatActiveTime(avgSeconds)}/day
        </span>
        <span className={`text-xs ${trendColor}`}>
          <TrendIcon />
        </span>
      </div>
    </div>
  )
}