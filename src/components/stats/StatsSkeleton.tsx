export default function StatsSkeleton() {
  return (
    <div className="space-y-5 animate-pulse">
      {/* Hero cards */}
      <div className="grid grid-cols-2 gap-3">
        <div className="bg-white rounded-xl p-4 shadow-sm h-32">
          <div className="w-9 h-9 rounded-lg bg-gray-200 mb-2" />
          <div className="w-16 h-7 bg-gray-200 rounded mb-1" />
          <div className="w-20 h-3 bg-gray-100 rounded" />
        </div>
        <div className="bg-white rounded-xl p-4 shadow-sm h-32">
          <div className="w-9 h-9 rounded-lg bg-gray-200 mb-2" />
          <div className="w-16 h-7 bg-gray-200 rounded mb-1" />
          <div className="w-20 h-3 bg-gray-100 rounded" />
        </div>
      </div>
      {/* Chart placeholders */}
      {[1, 2, 3].map((i) => (
        <div key={i} className="bg-white rounded-xl p-4 shadow-sm h-56">
          <div className="w-24 h-4 bg-gray-200 rounded mb-3" />
          <div className="w-full h-40 bg-gray-100 rounded" />
        </div>
      ))}
    </div>
  )
}