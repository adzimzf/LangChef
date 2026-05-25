import { FireOutlined } from '@ant-design/icons'

interface Props {
  streak: number
  longestStreak: number
  recentActivity: { date: string; hasActivity: boolean }[]
}

export default function StreakCard({ streak, longestStreak, recentActivity }: Props) {
  return (
    <div className="bg-white rounded-xl p-4 shadow-sm">
      <div className="inline-flex items-center justify-center w-9 h-9 rounded-lg bg-orange-50 text-orange-500 text-lg mb-2">
        <FireOutlined />
      </div>
      <div className="flex items-baseline gap-1">
        <span className="text-3xl font-bold">{streak}</span>
        <span className="text-xs text-gray-400">day streak</span>
      </div>
      <div className="text-xs text-gray-400 mt-1">
        Longest: {longestStreak} days
      </div>
      <div className="flex gap-1.5 mt-3">
        {recentActivity.map((d) => (
          <div
            key={d.date}
            className={`w-7 h-7 rounded-md ${
              d.hasActivity ? 'bg-orange-400' : 'bg-gray-100'
            }`}
            title={d.date}
          />
        ))}
      </div>
    </div>
  )
}