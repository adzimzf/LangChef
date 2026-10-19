import { CheckCircleFilled, CloseCircleFilled, ExclamationCircleFilled, ReloadOutlined, ReadOutlined, PlusOutlined } from '@ant-design/icons'
import { t } from '../../lib/i18n'
import type { AnswerStats, ReadingQuestion, SavedAnswer } from '../../lib/types'

interface Props {
  title: string
  questions: ReadingQuestion[]
  answers: SavedAnswer[]
  stats: AnswerStats
  elapsedSeconds: number
  onReadAgain: () => void
  onAnswerAgain: () => void
  onReadAnother: () => void
}

export function formatTime(seconds: number): string {
  const m = Math.floor(seconds / 60)
  const s = seconds % 60
  return m > 0 ? t('reading.minSec', { m, s }) : t('reading.sec', { s })
}

export const RESULT_STYLE: Record<string, { icon: React.ReactNode; badge: string; label: string }> = {
  correct: { icon: <CheckCircleFilled className="text-emerald-500" />, badge: 'bg-emerald-50 text-emerald-700', label: 'reading.correct' },
  partial: { icon: <ExclamationCircleFilled className="text-amber-500" />, badge: 'bg-amber-50 text-amber-700', label: 'reading.partial' },
  incorrect: { icon: <CloseCircleFilled className="text-rose-500" />, badge: 'bg-rose-50 text-rose-700', label: 'reading.skipped' },
}

/** Ring showing correct answers out of all questions. */
function ScoreRing({ correct, total }: { correct: number; total: number }) {
  const r = 52
  const c = 2 * Math.PI * r
  const share = total > 0 ? correct / total : 0
  return (
    <div className="relative w-36 h-36">
      <svg viewBox="0 0 120 120" className="w-full h-full -rotate-90">
        <circle cx="60" cy="60" r={r} fill="none" stroke="#e0e7ff" strokeWidth="10" />
        <circle
          cx="60" cy="60" r={r} fill="none" stroke="#4f46e5" strokeWidth="10" strokeLinecap="round"
          strokeDasharray={c} strokeDashoffset={c * (1 - share)}
          style={{ transition: 'stroke-dashoffset 0.8s ease-out' }}
        />
      </svg>
      <div className="absolute inset-0 flex flex-col items-center justify-center">
        <span className="text-3xl font-bold text-gray-900">{correct}<span className="text-gray-300">/{total}</span></span>
        <span className="text-[11px] uppercase tracking-wide text-gray-400 font-semibold">{t('reading.correctLabel')}</span>
      </div>
    </div>
  )
}

/** End of an article: score, time, and every question with the learner's answer and the feedback. */
export default function ReadingSummary({
  title, questions, answers, stats, elapsedSeconds, onReadAgain, onAnswerAgain, onReadAnother,
}: Props) {
  const total = questions.length
  const headline = stats.correct === total ? 'reading.resultGreat' : stats.correct * 2 >= total ? 'reading.resultGood' : 'reading.resultKeep'

  return (
    <div className="flex flex-col gap-5 pb-6">
      <div className="rounded-3xl bg-gradient-to-b from-indigo-50 to-white border border-indigo-100 px-5 pt-6 pb-5 flex flex-col items-center text-center">
        <ScoreRing correct={stats.correct} total={total} />
        <h2 className="text-xl font-bold text-gray-900 mt-3">{t(headline)}</h2>
        <p className="text-sm text-gray-500 mt-1 line-clamp-2">{title}</p>

        <div className="w-full grid grid-cols-3 gap-2 mt-5">
          {[
            { value: String(stats.first_try), label: t('reading.firstTry'), color: 'text-emerald-600' },
            { value: String(stats.retries), label: t('reading.retries'), color: 'text-amber-600' },
            { value: elapsedSeconds > 0 ? formatTime(elapsedSeconds) : '–', label: t('reading.time'), color: 'text-indigo-600' },
          ].map((tile) => (
            <div key={tile.label} className="bg-white rounded-2xl border border-gray-100 py-3 px-1 flex flex-col items-center">
              <span className={`text-lg font-bold ${tile.color}`}>{tile.value}</span>
              <span className="text-[11px] text-gray-500 mt-0.5">{tile.label}</span>
            </div>
          ))}
        </div>
      </div>

      <div>
        <p className="text-xs font-semibold text-gray-400 uppercase tracking-wide mb-2 px-1">{t('reading.reviewAnswers')}</p>
        <div className="flex flex-col gap-2">
          {questions.map((q, i) => {
            const a = answers.find((x) => x.question_index === i)
            const style = RESULT_STYLE[a?.result ?? 'incorrect'] ?? RESULT_STYLE.incorrect
            return (
              <div key={i} className="bg-white rounded-2xl border border-gray-100 px-4 py-3">
                <div className="flex items-start gap-2.5">
                  <span className="text-base mt-0.5 shrink-0">{style.icon}</span>
                  <div className="flex-1 min-w-0">
                    <p className="text-[15px] font-medium text-gray-800 leading-snug">{q.question}</p>
                    {a?.user_answer && (
                      <p className="text-sm text-gray-600 mt-1.5">
                        <span className="text-gray-400">{t('reading.yourAnswer')}: </span>{a.user_answer}
                      </p>
                    )}
                    {a?.feedback && <p className="text-xs text-gray-500 mt-1.5 leading-relaxed">{a.feedback}</p>}
                  </div>
                  <span className={`shrink-0 text-[11px] font-semibold px-2 py-0.5 rounded-full ${style.badge}`}>{t(style.label)}</span>
                </div>
              </div>
            )
          })}
        </div>
      </div>

      <div className="flex flex-col gap-2">
        <button
          onClick={onReadAnother}
          className="w-full py-3.5 rounded-2xl bg-indigo-600 text-white font-semibold text-[15px] active:bg-indigo-700 flex items-center justify-center gap-2"
        >
          <PlusOutlined /> {t('reading.readAnother')}
        </button>
        <div className="grid grid-cols-2 gap-2">
          <button
            onClick={onReadAgain}
            className="py-3 rounded-2xl bg-white border border-gray-200 text-gray-700 font-medium text-sm active:bg-gray-50 flex items-center justify-center gap-2"
          >
            <ReadOutlined /> {t('reading.readAgain')}
          </button>
          <button
            onClick={onAnswerAgain}
            className="py-3 rounded-2xl bg-white border border-gray-200 text-gray-700 font-medium text-sm active:bg-gray-50 flex items-center justify-center gap-2"
          >
            <ReloadOutlined /> {t('reading.answerAgain')}
          </button>
        </div>
      </div>
    </div>
  )
}
