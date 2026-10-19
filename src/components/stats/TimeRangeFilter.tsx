import { Segmented } from 'antd'
import type { TimeRange } from '../../lib/stats'
import { t } from '../../lib/i18n'

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
        { label: t('stats.weekly'), value: 'weekly' },
        { label: t('stats.monthly'), value: 'monthly' },
        { label: t('stats.yearly'), value: 'yearly' },
      ]}
      block
    />
  )
}