import { Segmented } from 'antd'
import type { TimeRange } from '../../lib/stats'

interface Props {
  value: TimeRange
  onChange: (range: TimeRange) => void
}

export default function TimeRangeFilter({ value, onChange }: Props) {
  return (
    <Segmented
      value={value}
      onChange={(val) => onChange(val as TimeRange)}
      options={[
        { label: 'Weekly', value: 'weekly' },
        { label: 'Monthly', value: 'monthly' },
        { label: 'Yearly', value: 'yearly' },
      ]}
      block
    />
  )
}