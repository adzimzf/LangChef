import { CheckCircleOutlined } from '@ant-design/icons'
import { t } from '../../lib/i18n'

interface Props {
  totalQuestions: number
  totalRetries: number
  elapsedSeconds: number
  onReadAnother: () => void
}

function formatTime(seconds: number): string {
  const m = Math.floor(seconds / 60)
  const s = seconds % 60
  return m > 0 ? t('reading.minSec', { m, s }) : t('reading.sec', { s })
}

export default function ReadingSummary({ totalQuestions, totalRetries, elapsedSeconds, onReadAnother }: Props) {
  return (
    <div className="flex flex-col items-center gap-6 py-8">
      <div className="w-20 h-20 rounded-full bg-green-100 flex items-center justify-center">
        <CheckCircleOutlined className="text-green-500 text-4xl" />
      </div>

      <div className="text-center">
        <h2 className="text-xl font-bold text-gray-800">{t('reading.completeTitle')}</h2>
        <p className="text-sm text-gray-500 mt-1">{t('reading.completeBody')}</p>
      </div>

      <div className="w-full grid grid-cols-3 gap-3">
        <div className="flex flex-col items-center bg-blue-50 rounded-2xl py-4 px-2">
          <span className="text-2xl font-bold text-blue-600">{totalQuestions}</span>
          <span className="text-xs text-gray-500 mt-1 text-center">{t('reading.questions')}</span>
        </div>
        <div className="flex flex-col items-center bg-orange-50 rounded-2xl py-4 px-2">
          <span className="text-2xl font-bold text-orange-500">{totalRetries}</span>
          <span className="text-xs text-gray-500 mt-1 text-center">{t('reading.retries')}</span>
        </div>
        <div className="flex flex-col items-center bg-purple-50 rounded-2xl py-4 px-2">
          <span className="text-2xl font-bold text-purple-600">{formatTime(elapsedSeconds)}</span>
          <span className="text-xs text-gray-500 mt-1 text-center">{t('reading.time')}</span>
        </div>
      </div>

      <button
        onClick={onReadAnother}
        className="w-full py-3.5 rounded-xl bg-blue-500 text-white font-semibold text-[15px] active:bg-blue-600"
      >
        {t('reading.readAnother')}
      </button>
    </div>
  )
}
