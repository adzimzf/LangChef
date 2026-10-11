import { useEffect, useRef, useState } from 'react'
import type { ReactNode } from 'react'
import { BulbOutlined } from '@ant-design/icons'
import { NONE_ANSWER } from '../lib/types'
import type { AnswerResult, QuizAnswer, QuizTask, SessionSummary } from '../lib/types'

export const GRADE_COLORS: Record<string, string> = {
  again: 'bg-red-500',
  hard: 'bg-orange-500',
  good: 'bg-blue-500',
  easy: 'bg-green-500',
}

const VERDICT_BG: Record<string, string> = {
  again: 'bg-red-50',
  hard: 'bg-orange-50',
  good: 'bg-blue-50',
  easy: 'bg-green-50',
}

const KIND_LABEL: Record<string, string> = {
  intro_tf: 'New word',
  true_false: 'True or false',
  choice: 'Pick the meaning',
  recall_confirm: 'Recall',
  reading: 'Pick the reading',
  context: 'In context',
}

const optionBase = 'w-full text-left px-4 py-3 rounded-2xl bg-white border-[1.5px] border-gray-200 text-sm font-medium hover:border-blue-300 active:bg-gray-50 transition-colors disabled:opacity-60'
const secondaryBtn = 'py-3 rounded-2xl bg-gray-100 text-gray-700 text-sm font-medium hover:bg-gray-200 active:bg-gray-300 transition-colors disabled:opacity-50'
const primaryBtn = 'py-3 rounded-2xl bg-blue-500 text-white text-sm font-medium hover:bg-blue-600 active:bg-blue-700 transition-colors shadow-sm disabled:opacity-50'

/** Word with furigana. The reading's space is always kept so revealing it doesn't move the buttons. */
function Furigana({ word, reading, show }: { word: string; reading: string; show: boolean }) {
  if (!reading || reading === word) return <>{word}</>
  return (
    <ruby>{word}<rt className={`text-xs font-normal text-gray-400 ${show ? '' : 'invisible'}`}>{reading}</rt></ruby>
  )
}

function Word({ word, reading, showReading, size = 'text-4xl' }: { word: string; reading: string; showReading: boolean; size?: string }) {
  return (
    <div className={`${size} font-semibold leading-tight`}>
      <Furigana word={word} reading={reading} show={showReading} />
    </div>
  )
}

/** Fills over 10 seconds so the user feels the clock without a number to stare at. */
function TimerBar({ startedAt }: { startedAt: number }) {
  const [pct, setPct] = useState(0)
  useEffect(() => {
    let raf = 0
    const tick = () => {
      setPct(Math.min(100, (performance.now() - startedAt) / 100))
      raf = requestAnimationFrame(tick)
    }
    tick()
    return () => cancelAnimationFrame(raf)
  }, [startedAt])
  return (
    <div className="h-1 bg-gray-100 rounded-full overflow-hidden">
      <div className="h-full bg-gradient-to-r from-green-400 via-blue-400 to-orange-400" style={{ width: `${pct}%` }} />
    </div>
  )
}

type Phase = 'intro' | 'recall' | 'answer'

/** Renders one question and reports what the user did. Grading happens in Rust. */
export function ReviewQuiz({ task, submitting, onAnswer }: {
  task: QuizTask
  submitting: boolean
  onAnswer: (answer: QuizAnswer) => void
}) {
  const initialPhase: Phase = task.kind === 'intro_tf' ? 'intro' : task.kind === 'recall_confirm' ? 'recall' : 'answer'
  const [phase, setPhase] = useState<Phase>(initialPhase)
  const [startedAt, setStartedAt] = useState(() => performance.now())
  const [recallMs, setRecallMs] = useState<number | null>(null)
  const [hintUsed, setHintUsed] = useState(false)

  const elapsed = () => Math.round(performance.now() - startedAt)
  const send = (a: Omit<QuizAnswer, 'hint_used' | 'elapsed_ms'>, ms?: number) =>
    onAnswer({ ...a, elapsed_ms: ms ?? elapsed(), hint_used: hintUsed })

  // Kept in the layout after use (invisible) so the answer buttons don't move under the thumb.
  const hintButton = task.hint_available && phase !== 'intro' && (
    <button
      type="button"
      disabled={hintUsed}
      onClick={() => setHintUsed(true)}
      className={`flex items-center justify-center gap-1.5 py-2 text-xs text-gray-400 hover:text-gray-600 ${hintUsed ? 'invisible' : ''}`}
    >
      <BulbOutlined /> Show furigana (counts as a hint)
    </button>
  )
  const dontKnow = (
    <button type="button" disabled={submitting} onClick={() => send({ dont_know: true }, recallMs ?? undefined)} className="py-2 text-sm text-gray-400 hover:text-gray-600">
      Don't know
    </button>
  )

  const isTf = task.kind === 'intro_tf' || task.kind === 'true_false'
  const showMeaningOnCard = task.kind === 'reading' || phase === 'intro'
  // Recall → confirm is timed only up to "I know it"; the confirm tap isn't part of recall.
  const showTimer = phase === 'recall' || (phase === 'answer' && task.kind !== 'recall_confirm')

  return (
    <div className="flex flex-col gap-3 w-full max-w-sm mx-auto">
      <div className="bg-white rounded-2xl shadow-md px-6 py-8 text-center flex flex-col items-center gap-2">
        <span className="text-[10px] font-bold uppercase tracking-wider px-2 py-0.5 rounded-full bg-blue-50 text-blue-500">
          {KIND_LABEL[task.kind]}{task.mature ? ' · mature' : ''}
        </span>
        {task.kind === 'context' && task.sentence ? (
          <div className="text-xl leading-loose">
            {task.sentence.before}
            <mark className="bg-blue-50 text-blue-600 rounded px-1">
              <Furigana word={task.sentence.target} reading={task.reading} show={hintUsed} />
            </mark>
            {task.sentence.after}
          </div>
        ) : (
          <Word word={task.word} reading={task.kind === 'reading' ? '' : task.reading} showReading={hintUsed || phase === 'intro'} />
        )}
        {showMeaningOnCard && <div className="text-sm text-gray-500">{task.meaning}</div>}
        {isTf && phase === 'answer' && task.statement && (
          <>
            <span className="text-xs text-gray-400">means</span>
            <span className="px-3 py-1.5 rounded-xl bg-blue-50 text-blue-600 font-semibold">{task.statement.text}</span>
          </>
        )}
        {phase === 'recall' && <span className="text-xs text-gray-400 mt-1">Think of the meaning, then tap</span>}
        {task.kind === 'context' && <span className="text-xs text-gray-400 mt-1">What does the highlighted word mean here?</span>}
        {phase === 'answer' && task.kind === 'recall_confirm' && <span className="text-xs text-gray-400 mt-1">Which meaning did you recall?</span>}
      </div>

      {showTimer && <TimerBar startedAt={startedAt} />}

      {phase === 'intro' && (
        <button type="button" className={primaryBtn} onClick={() => { setPhase('answer'); setStartedAt(performance.now()) }}>
          Got it
        </button>
      )}

      {phase === 'recall' && (
        <>
          <div className="grid grid-cols-2 gap-2">
            <button type="button" disabled={submitting} className={secondaryBtn} onClick={() => send({ dont_know: true })}>Don't know</button>
            <button type="button" className={primaryBtn} onClick={() => { setRecallMs(elapsed()); setPhase('answer') }}>I know it</button>
          </div>
          {hintButton}
        </>
      )}

      {phase === 'answer' && isTf && (
        <div className="grid grid-cols-2 gap-2">
          <button type="button" disabled={submitting} className={secondaryBtn} onClick={() => send({ dont_know: false, tf: false })}>✕ Wrong</button>
          <button type="button" disabled={submitting} className={secondaryBtn} onClick={() => send({ dont_know: false, tf: true })}>✓ Right</button>
          <div className="col-span-2 flex justify-center">{hintButton}</div>
        </div>
      )}

      {phase === 'answer' && !isTf && (
        <>
          <div className={task.kind === 'reading' ? 'grid grid-cols-2 gap-2' : 'flex flex-col gap-2'}>
            {task.options.map(o => (
              <button
                key={o.text}
                type="button"
                disabled={submitting}
                className={`${optionBase} ${task.kind === 'reading' ? 'text-center text-base' : ''}`}
                onClick={() => send({ dont_know: false, choice: o.text }, recallMs ?? undefined)}
              >
                {o.text}
              </button>
            ))}
          </div>
          {task.kind === 'recall_confirm' ? (
            <button type="button" disabled={submitting} className="py-2 text-sm text-gray-500 hover:text-gray-700" onClick={() => send({ dont_know: false, choice: NONE_ANSWER }, recallMs ?? undefined)}>
              None of these
            </button>
          ) : dontKnow}
          {task.kind !== 'recall_confirm' && hintButton}
        </>
      )}
    </div>
  )
}

/** Result of an answer: what was right, the grade the backend picked and why. */
export function QuizVerdictView({ task, result, details, onNext, onOverride, overriding }: {
  task: QuizTask
  result: AnswerResult
  details: ReactNode
  onNext: () => void
  onOverride: () => void
  overriding: boolean
}) {
  const v = result.verdict
  const [showDetails, setShowDetails] = useState(false)
  const chosenLabel = v.chosen === NONE_ANSWER ? 'None of these' : v.chosen === 'true' ? '✓ Right' : v.chosen === 'false' ? '✕ Wrong' : v.chosen
  const isTf = task.kind === 'intro_tf' || task.kind === 'true_false'

  return (
    <div className="flex flex-col gap-3 w-full max-w-sm mx-auto">
      <div className="bg-white rounded-2xl shadow-md px-6 py-6 text-center flex flex-col items-center gap-1">
        <Word word={task.word} reading={task.reading} showReading size="text-3xl" />
        <div className="text-sm text-gray-500">{task.meaning}</div>
        {task.kind === 'context' && task.sentence?.translation && (
          <div className="text-xs text-gray-400 mt-1">{task.sentence.translation}</div>
        )}
      </div>

      <div className="flex flex-col gap-2">
        {isTf && task.statement && (
          <div className="px-4 py-3 rounded-2xl bg-white border-[1.5px] border-gray-200 text-sm flex justify-between gap-2">
            <span>“{task.statement.text}”</span>
            <span className={task.statement.text === task.meaning ? 'text-green-600' : 'text-red-500'}>
              {task.statement.text === task.meaning ? 'is right' : 'is wrong'}
            </span>
          </div>
        )}
        {!v.correct && !v.dont_know && chosenLabel && (
          <div className="px-4 py-3 rounded-2xl border-[1.5px] border-red-400 bg-red-50 text-sm font-medium flex justify-between">
            <span>{chosenLabel}</span><span className="text-xs text-gray-400">your pick</span>
          </div>
        )}
        {!isTf && (
          <div className="px-4 py-3 rounded-2xl border-[1.5px] border-green-500 bg-green-50 text-sm font-medium flex justify-between">
            <span>{v.correct_answer}</span><span>✓</span>
          </div>
        )}
      </div>

      <div className={`rounded-2xl p-3 flex flex-col gap-2 ${VERDICT_BG[v.grade] || 'bg-gray-50'}`}>
        <div className="flex items-center justify-between">
          <span className={`px-2.5 py-0.5 rounded-full text-white text-xs font-bold ${GRADE_COLORS[v.grade]}`}>{v.label}</span>
          <span className="text-xs text-gray-500">next in {v.next_review_formatted}</span>
        </div>
        <div className="flex flex-wrap gap-1.5">
          {v.reasons.map(r => (
            <span key={r} className="text-[11px] px-2 py-0.5 rounded-full bg-white text-gray-500">{r}</span>
          ))}
        </div>
        {v.confused_with && (
          <div className="grid grid-cols-2 gap-2">
            <div className="bg-white rounded-xl p-2 text-center">
              <div className="text-lg">{task.word}</div>
              <div className="text-[11px] text-gray-500">{task.reading ? `${task.reading} · ` : ''}{task.meaning}</div>
            </div>
            <div className="bg-white rounded-xl p-2 text-center">
              <div className="text-lg">{v.confused_with.word}</div>
              <div className="text-[11px] text-gray-500">{v.confused_with.reading ? `${v.confused_with.reading} · ` : ''}{v.confused_with.meaning}</div>
            </div>
          </div>
        )}
      </div>

      {v.can_override && (
        <button type="button" disabled={overriding} onClick={onOverride} className="py-1 text-xs text-gray-400 hover:text-gray-600">
          {overriding ? 'Fixing…' : 'I mis-tapped (1 per day)'}
        </button>
      )}

      <button type="button" onClick={() => setShowDetails(s => !s)} className="py-1 text-xs text-gray-400 hover:text-gray-600">
        {showDetails ? 'Hide details' : 'Show details'}
      </button>
      {showDetails && <div className="bg-white rounded-2xl shadow-sm px-4 pb-4">{details}</div>}

      <button type="button" className={primaryBtn} onClick={onNext}>
        {result.next_card ? 'Next →' : 'Finish'}
      </button>
    </div>
  )
}

/** Moves on after a correct answer unless the user touches the result screen. */
export function useAutoAdvance(result: AnswerResult | null, onNext: () => void, delayMs = 1500) {
  const cancelled = useRef(false)
  const next = useRef(onNext)
  next.current = onNext
  useEffect(() => {
    cancelled.current = false
    if (!result || !result.verdict.correct || result.verdict.grade === 'hard') return
    const t = setTimeout(() => { if (!cancelled.current) next.current() }, delayMs)
    return () => clearTimeout(t)
  }, [result, delayMs])
  return () => { cancelled.current = true }
}

export function SessionSummaryView({ summary }: { summary: SessionSummary }) {
  const grades: { key: keyof SessionSummary; label: string; color: string }[] = [
    { key: 'again', label: 'Again', color: GRADE_COLORS.again },
    { key: 'hard', label: 'Hard', color: GRADE_COLORS.hard },
    { key: 'good', label: 'Good', color: GRADE_COLORS.good },
    { key: 'easy', label: 'Easy', color: GRADE_COLORS.easy },
  ]
  const stat = (value: string, label: string) => (
    <div className="bg-white rounded-xl py-2 text-center">
      <div className="text-lg font-semibold tabular-nums">{value}</div>
      <div className="text-[10px] text-gray-400">{label}</div>
    </div>
  )
  const listCard = (title: string, rows: { left: string; right: string; tone: string }[]) => rows.length > 0 && (
    <div className="bg-white rounded-xl px-3 py-2 flex flex-col gap-1 text-sm">
      <div className="text-[11px] text-gray-400">{title}</div>
      {rows.map(r => (
        <div key={r.left + r.right} className="flex justify-between gap-2">
          <span>{r.left}</span><span className={r.tone}>{r.right}</span>
        </div>
      ))}
    </div>
  )

  return (
    <div className="w-full max-w-sm flex flex-col gap-2 text-gray-700">
      <div className="grid grid-cols-3 gap-2">
        {stat(`${summary.correct}/${summary.total}`, 'correct')}
        {stat(summary.avg_ms != null ? `${(summary.avg_ms / 1000).toFixed(1)}s` : '—', 'avg answer')}
        {summary.mature_retention != null
          ? stat(`${summary.mature_retention}%`, 'mature kept')
          : stat(String(summary.hints), summary.hints === 1 ? 'hint used' : 'hints used')}
      </div>
      <div className="bg-white rounded-xl px-3 py-2 flex flex-col gap-1.5">
        <div className="text-[11px] text-gray-400">Grades (set automatically)</div>
        <div className="flex h-2.5 rounded-full overflow-hidden bg-gray-100">
          {grades.map(g => {
            const n = summary[g.key] as number
            return n > 0 ? <div key={g.key} className={g.color} style={{ width: `${(n / summary.total) * 100}%` }} /> : null
          })}
        </div>
        <div className="flex justify-between text-[10px] text-gray-500">
          {grades.map(g => <span key={g.key}>{g.label} {summary[g.key] as number}</span>)}
        </div>
      </div>
      {summary.mature_retention != null && (
        <div className="bg-white rounded-xl px-3 py-2 text-sm flex justify-between">
          <span>Mature retention</span>
          <span className={summary.mature_retention >= 90 ? 'text-green-600' : 'text-orange-500'}>
            {summary.mature_retention}% · target 90%
          </span>
        </div>
      )}
      {listCard('Mixed up today', summary.mixed_up.map(m => ({ left: `${m.word} ↔ ${m.other}`, right: `${m.count}×`, tone: 'text-red-500' })))}
      {listCard('Reading slips', summary.reading_slips.map(s => ({ left: `${s.word} ${s.reading}`, right: `picked ${s.picked}`, tone: 'text-orange-500' })))}
      {listCard('Missed in context', summary.context_misses.map(m => ({ left: m.word, right: `picked ${m.other}'s meaning`, tone: 'text-red-500' })))}
    </div>
  )
}
