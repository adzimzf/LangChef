import { useState } from 'react'
import { CheckCircleOutlined, CloseCircleOutlined, InfoCircleOutlined, LoadingOutlined } from '@ant-design/icons'
import type { AnswerEvaluation } from '../../lib/types'

interface Props {
  question: string
  index: number
  total: number
  evaluation: AnswerEvaluation | null
  submitting: boolean
  onSubmit: (answer: string) => Promise<void>
  onNext: () => void
  onContinue: () => void
  onRetry: () => void
}

export default function QuestionCard({
  question, index, total, evaluation, submitting, onSubmit, onNext, onContinue, onRetry,
}: Props) {
  const [answer, setAnswer] = useState('')

  async function handleSubmit() {
    if (!answer.trim() || submitting) return
    await onSubmit(answer.trim())
  }

  function handleRetry() {
    setAnswer('')
    onRetry()
  }

  const resultStyle = evaluation
    ? evaluation.result === 'correct'
      ? { bg: 'bg-green-50 border-green-200', icon: <CheckCircleOutlined className="text-green-500" />, label: 'Correct!' }
      : evaluation.result === 'partial'
        ? { bg: 'bg-yellow-50 border-yellow-200', icon: <InfoCircleOutlined className="text-yellow-500" />, label: 'Almost there' }
        : { bg: 'bg-red-50 border-red-200', icon: <CloseCircleOutlined className="text-red-500" />, label: 'Not quite' }
    : null

  const isAnswered = !!evaluation
  const isCorrect = evaluation?.result === 'correct'
  const isPartial = evaluation?.result === 'partial'
  const isIncorrect = isAnswered && !isCorrect && !isPartial

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between">
        <span className="text-xs font-medium text-gray-400 uppercase tracking-wide">
          Question {index + 1} / {total}
        </span>
      </div>

      <p className="text-[16px] font-medium text-gray-800 leading-relaxed">{question}</p>

      <textarea
        className="w-full rounded-xl border border-gray-200 bg-gray-50 px-4 py-3 text-[15px]
          placeholder:text-gray-400 focus:outline-none focus:ring-2 focus:ring-blue-300
          resize-none disabled:opacity-60"
        rows={3}
        placeholder="Type your answer…"
        value={answer}
        onChange={(e) => setAnswer(e.target.value)}
        disabled={submitting || isCorrect}
        onKeyDown={(e) => { if (e.key === 'Enter' && e.metaKey) handleSubmit() }}
      />

      {resultStyle && evaluation && (
        <div className={`rounded-xl border px-4 py-3 flex gap-3 items-start ${resultStyle.bg}`}>
          <span className="mt-0.5 text-lg shrink-0">{resultStyle.icon}</span>
          <div className="flex-1">
            <p className="font-semibold text-sm">{resultStyle.label}</p>
            <p className="text-sm text-gray-700 mt-0.5">{evaluation.feedback}</p>
          </div>
        </div>
      )}

      <div className="flex gap-2">
        {!isAnswered && (
          <button
            onClick={handleSubmit}
            disabled={!answer.trim() || submitting}
            className="flex-1 py-3 rounded-xl bg-blue-500 text-white font-semibold text-[15px]
              disabled:opacity-40 active:bg-blue-600 flex items-center justify-center gap-2"
          >
            {submitting ? <><LoadingOutlined spin /> Checking…</> : 'Submit'}
          </button>
        )}
        {isCorrect && (
          <button
            onClick={onNext}
            className="flex-1 py-3 rounded-xl bg-green-500 text-white font-semibold text-[15px] active:bg-green-600"
          >
            {index + 1 < total ? 'Next Question →' : 'Finish'}
          </button>
        )}
        {isPartial && (
          <>
            <button
              onClick={handleRetry}
              className="flex-1 py-3 rounded-xl border border-yellow-300 bg-yellow-50 text-yellow-700
                font-semibold text-[15px] active:bg-yellow-100"
            >
              Try Again
            </button>
            <button
              onClick={onContinue}
              className="flex-1 py-3 rounded-xl bg-blue-500 text-white font-semibold text-[15px] active:bg-blue-600"
            >
              {index + 1 < total ? 'Continue →' : 'Finish'}
            </button>
          </>
        )}
        {isIncorrect && (
          <button
            onClick={handleRetry}
            className="flex-1 py-3 rounded-xl border border-red-200 bg-red-50 text-red-600
              font-semibold text-[15px] active:bg-red-100"
          >
            Try Again
          </button>
        )}
      </div>
    </div>
  )
}
