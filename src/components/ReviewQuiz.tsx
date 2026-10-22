import { useEffect, useRef, useState } from 'react'
import type { ReactNode } from 'react'
import { BulbOutlined } from '@ant-design/icons'
import { NONE_ANSWER } from '../lib/types'
import type { AnswerResult, CardDetail, GradeReason, GrammarInfo, QuizAnswer, QuizTask, SessionSummary } from '../lib/types'
import { t, lang as explainLang, formatInMinutes, formatSeconds, languageName, partOfSpeechLabel } from '../lib/i18n'

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
  intro_tf: t('quiz.kind.intro_tf'),
  true_false: t('quiz.kind.true_false'),
  choice: t('quiz.kind.choice'),
  recall_confirm: t('quiz.kind.recall_confirm'),
  reading: t('quiz.kind.reading'),
  context: t('quiz.kind.context'),
  grammar_meaning: t('quiz.kind.grammar_meaning'),
  grammar_gap: t('quiz.kind.grammar_gap'),
  grammar_mistake: t('quiz.kind.grammar_mistake'),
}

/** A grade reason from Rust, in the learner's language. */
function reasonText(r: GradeReason): string {
  return t(`reason.${r.code}`, { time: r.ms != null ? formatSeconds(r.ms) : '' })
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

type Sentence = NonNullable<QuizTask['sentence']>

/** A sentence with the pattern marked, or with a blank where it goes (`fill` fills the blank). */
function GrammarSentence({ sentence, fill }: { sentence: Sentence; fill?: string }) {
  const target = fill ?? sentence.target
  return (
    <div className="text-xl leading-loose">
      {sentence.before}
      {target
        ? <mark className="bg-blue-50 text-blue-600 rounded px-1">{target}</mark>
        : <span className="inline-block min-w-[4.5rem] mx-0.5 border-b-2 border-blue-400">{'\u00a0'}</span>}
      {sentence.after}
    </div>
  )
}

function Highlighted({ sentence, target }: { sentence: string; target: string }) {
  const at = target ? sentence.indexOf(target) : -1
  if (at < 0) return <>{sentence}</>
  return (
    <>
      {sentence.slice(0, at)}
      <mark className="bg-blue-50 text-blue-600 rounded px-0.5">{target}</mark>
      {sentence.slice(at + target.length)}
    </>
  )
}

/** What a grammar pattern means and how it is built: intro, result screen, card details. */
export function GrammarInfoView({ title, info }: { title?: string; info: GrammarInfo }) {
  return (
    <div className="flex flex-col gap-2 text-left w-full">
      {title && <div className="text-2xl font-semibold text-center">{title}</div>}
      <div className={`text-sm text-gray-700 ${title ? 'text-center' : ''}`}>{info.meaning}</div>
      {info.form && (
        <div className="bg-gray-50 rounded-xl px-3 py-2">
          <div className="text-[11px] text-gray-400">{t('grammarCard.form')}</div>
          <div className="text-sm">{info.form}</div>
        </div>
      )}
      {info.examples.map(e => (
        <div key={e.sentence} className="flex flex-col gap-0.5">
          <div className="text-base"><Highlighted sentence={e.sentence} target={e.target} /></div>
          <div className="text-xs text-gray-500">{e.translation}</div>
        </div>
      ))}
      {info.note && <div className="text-xs text-gray-500">{info.note}</div>}
    </div>
  )
}

/** "Grammar · N4", or "Level check · N5" for a pattern below the learner's level. */
function GrammarChip({ task }: { task: QuizTask }) {
  if (!task.grammar) return null
  const level = task.grammar.level
  const text = task.placement
    ? (level ? t('quiz.levelCheckChip', { level }) : t('quiz.levelCheck'))
    : (level ? t('quiz.grammarChip', { level }) : t('quiz.grammar'))
  return (
    <span className={`text-[10px] font-bold uppercase tracking-wider px-2 py-0.5 rounded-full ${task.placement ? 'bg-amber-50 text-amber-700' : 'bg-indigo-50 text-indigo-600'}`}>
      {text}
    </span>
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
      <BulbOutlined /> {t('quiz.showFurigana')}
    </button>
  )
  const dontKnow = (
    <button type="button" disabled={submitting} onClick={() => send({ dont_know: true }, recallMs ?? undefined)} className="py-2 text-sm text-gray-400 hover:text-gray-600">
      {t('quiz.dontKnow')}
    </button>
  )

  const isTf = task.kind === 'intro_tf' || task.kind === 'true_false'
  const isGrammar = !!task.grammar
  const showMeaningOnCard = (task.kind === 'reading' || phase === 'intro') && !isGrammar
  const twoColumns = task.kind === 'reading' || task.kind === 'grammar_gap'
  // Recall → confirm is timed only up to "I know it"; the confirm tap isn't part of recall.
  const showTimer = phase === 'recall' || (phase === 'answer' && task.kind !== 'recall_confirm')

  return (
    <div className="flex flex-col gap-3 w-full max-w-sm mx-auto">
      <div className="bg-white rounded-2xl shadow-md px-6 py-8 text-center flex flex-col items-center gap-2">
        <div className="flex flex-wrap justify-center gap-1.5">
          <GrammarChip task={task} />
          <span className="text-[10px] font-bold uppercase tracking-wider px-2 py-0.5 rounded-full bg-blue-50 text-blue-500">
            {KIND_LABEL[task.kind]}{task.mature ? ` · ${t('quiz.mature')}` : ''}
          </span>
        </div>
        {phase === 'intro' && task.grammar ? (
          <GrammarInfoView title={task.word} info={task.grammar} />
        ) : (task.kind === 'grammar_meaning' || task.kind === 'grammar_gap') && task.sentence ? (
          <GrammarSentence sentence={task.sentence} />
        ) : task.kind === 'grammar_mistake' ? (
          <div className="flex flex-wrap justify-center gap-1.5">
            {task.options.map(o => (
              <button
                key={o.text}
                type="button"
                disabled={submitting}
                onClick={() => send({ dont_know: false, choice: o.text })}
                className="px-3 py-2 rounded-xl bg-white border-[1.5px] border-gray-200 text-xl hover:border-blue-300 active:bg-gray-50 disabled:opacity-60"
              >
                {o.text}
              </button>
            ))}
          </div>
        ) : task.kind === 'context' && task.sentence ? (
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
            <span className="text-xs text-gray-400">{t('quiz.means')}</span>
            <span className="px-3 py-1.5 rounded-xl bg-blue-50 text-blue-600 font-semibold">{task.statement.text}</span>
          </>
        )}
        {phase === 'recall' && <span className="text-xs text-gray-400 mt-1">{t('quiz.thinkThenTap')}</span>}
        {task.kind === 'context' && <span className="text-xs text-gray-400 mt-1">{t('quiz.contextQuestion')}</span>}
        {(task.kind === 'grammar_gap' || task.kind === 'grammar_mistake') && task.sentence?.translation && (
          <span className="text-sm text-gray-500">“{task.sentence.translation}”</span>
        )}
        {task.kind === 'grammar_meaning' && <span className="text-xs text-gray-400 mt-1">{t('quiz.grammarMeaningQuestion')}</span>}
        {task.kind === 'grammar_gap' && <span className="text-xs text-gray-400 mt-1">{t('quiz.gapQuestion')}</span>}
        {task.kind === 'grammar_mistake' && <span className="text-xs text-gray-400 mt-1">{t('quiz.mistakeQuestion')}</span>}
        {phase === 'answer' && task.kind === 'recall_confirm' && <span className="text-xs text-gray-400 mt-1">{t('quiz.whichRecalled')}</span>}
      </div>

      {task.placement && phase === 'answer' && <p className="text-xs text-center text-gray-500">{t('quiz.levelCheckHint')}</p>}

      {showTimer && <TimerBar startedAt={startedAt} />}

      {phase === 'intro' && (
        <button type="button" className={primaryBtn} onClick={() => { setPhase('answer'); setStartedAt(performance.now()) }}>
          {t('quiz.gotIt')}
        </button>
      )}

      {phase === 'recall' && (
        <>
          <div className="grid grid-cols-2 gap-2">
            <button type="button" disabled={submitting} className={secondaryBtn} onClick={() => send({ dont_know: true })}>{t('quiz.dontKnow')}</button>
            <button type="button" className={primaryBtn} onClick={() => { setRecallMs(elapsed()); setPhase('answer') }}>{t('quiz.iKnowIt')}</button>
          </div>
          {hintButton}
        </>
      )}

      {phase === 'answer' && isTf && (
        <div className="grid grid-cols-2 gap-2">
          <button type="button" disabled={submitting} className={secondaryBtn} onClick={() => send({ dont_know: false, tf: false })}>✕ {t('quiz.wrong')}</button>
          <button type="button" disabled={submitting} className={secondaryBtn} onClick={() => send({ dont_know: false, tf: true })}>✓ {t('quiz.right')}</button>
          <div className="col-span-2 flex justify-center">{hintButton}</div>
        </div>
      )}

      {phase === 'answer' && !isTf && (
        <>
          {task.kind !== 'grammar_mistake' && (
            <div className={twoColumns ? 'grid grid-cols-2 gap-2' : 'flex flex-col gap-2'}>
              {task.options.map(o => (
                <button
                  key={o.text}
                  type="button"
                  disabled={submitting}
                  className={`${optionBase} ${twoColumns ? 'text-center text-base' : ''}`}
                  onClick={() => send({ dont_know: false, choice: o.text }, recallMs ?? undefined)}
                >
                  {o.text}
                </button>
              ))}
            </div>
          )}
          {task.kind === 'recall_confirm' ? (
            <button type="button" disabled={submitting} className="py-2 text-sm text-gray-500 hover:text-gray-700" onClick={() => send({ dont_know: false, choice: NONE_ANSWER }, recallMs ?? undefined)}>
              {t('quiz.noneOfThese')}
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
  const chosenLabel = v.chosen === NONE_ANSWER ? t('quiz.noneOfThese') : v.chosen === 'true' ? `✓ ${t('quiz.right')}` : v.chosen === 'false' ? `✕ ${t('quiz.wrong')}` : v.chosen
  const isTf = task.kind === 'intro_tf' || task.kind === 'true_false'

  return (
    <div className="flex flex-col gap-3 w-full max-w-sm mx-auto">
      <div className="bg-white rounded-2xl shadow-md px-6 py-6 text-center flex flex-col items-center gap-1">
        <GrammarChip task={task} />
        <Word word={task.word} reading={task.reading} showReading size="text-3xl" />
        <div className="text-sm text-gray-500">{task.meaning}</div>
        {(task.kind === 'grammar_meaning' || task.kind === 'grammar_gap') && task.sentence && (
          <div className="mt-2">
            <GrammarSentence sentence={task.sentence} fill={task.kind === 'grammar_gap' ? v.correct_answer : undefined} />
          </div>
        )}
        {(task.kind === 'context' || task.kind === 'grammar_meaning' || task.kind === 'grammar_gap' || task.kind === 'grammar_mistake') && task.sentence?.translation && (
          <div className="text-xs text-gray-400 mt-1">{task.sentence.translation}</div>
        )}
      </div>

      <div className="flex flex-col gap-2">
        {isTf && task.statement && (
          <div className="px-4 py-3 rounded-2xl bg-white border-[1.5px] border-gray-200 text-sm flex justify-between gap-2">
            <span>“{task.statement.text}”</span>
            <span className={task.statement.text === task.meaning ? 'text-green-600' : 'text-red-500'}>
              {task.statement.text === task.meaning ? t('quiz.isRight') : t('quiz.isWrong')}
            </span>
          </div>
        )}
        {!v.correct && !v.dont_know && chosenLabel && (
          <div className="px-4 py-3 rounded-2xl border-[1.5px] border-red-400 bg-red-50 text-sm font-medium flex justify-between">
            <span>{chosenLabel}</span><span className="text-xs text-gray-400">{t('quiz.yourPick')}</span>
          </div>
        )}
        {!isTf && (
          <div className="px-4 py-3 rounded-2xl border-[1.5px] border-green-500 bg-green-50 text-sm font-medium flex justify-between">
            <span>{v.none_correct ? t('quiz.noneCorrect', { meaning: v.correct_answer }) : v.correct_answer}</span><span>✓</span>
          </div>
        )}
      </div>

      <div className={`rounded-2xl p-3 flex flex-col gap-2 ${VERDICT_BG[v.grade] || 'bg-gray-50'}`}>
        <div className="flex items-center justify-between">
          <span className={`px-2.5 py-0.5 rounded-full text-white text-xs font-bold ${GRADE_COLORS[v.grade]}`}>{t(`grade.${v.grade}`)}</span>
          <span className="text-xs text-gray-500">{t('quiz.nextIn', { when: formatInMinutes(v.next_minutes) })}</span>
        </div>
        <div className="flex flex-wrap gap-1.5">
          {v.reasons.map(r => (
            <span key={r.code} className="text-[11px] px-2 py-0.5 rounded-full bg-white text-gray-500">{reasonText(r)}</span>
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

      {task.placement && (
        <p className="text-xs text-center text-gray-600">{v.correct ? t('quiz.checkPassed') : t('quiz.checkMissed')}</p>
      )}
      {task.grammar && !v.correct && (
        <div className="bg-white rounded-2xl shadow-sm px-4 py-3">
          <GrammarInfoView info={task.grammar} />
        </div>
      )}

      {v.can_override && (
        <button type="button" disabled={overriding} onClick={onOverride} className="py-1 text-xs text-gray-400 hover:text-gray-600">
          {overriding ? t('quiz.fixing') : t('quiz.mistap')}
        </button>
      )}

      <button type="button" onClick={() => setShowDetails(s => !s)} className="py-1 text-xs text-gray-400 hover:text-gray-600">
        {showDetails ? t('quiz.hideDetails') : t('quiz.showDetails')}
      </button>
      {showDetails && <div className="bg-white rounded-2xl shadow-sm px-4 pb-4">{details}</div>}

      <button type="button" className={primaryBtn} onClick={onNext}>
        {result.next_card ? `${t('common.next')} →` : t('quiz.finish')}
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
    { key: 'again', label: t('grade.again'), color: GRADE_COLORS.again },
    { key: 'hard', label: t('grade.hard'), color: GRADE_COLORS.hard },
    { key: 'good', label: t('grade.good'), color: GRADE_COLORS.good },
    { key: 'easy', label: t('grade.easy'), color: GRADE_COLORS.easy },
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
        {stat(`${summary.correct}/${summary.total}`, t('summary.correct'))}
        {stat(summary.avg_ms != null ? formatSeconds(summary.avg_ms) : '—', t('summary.avgAnswer'))}
        {summary.mature_retention != null
          ? stat(`${summary.mature_retention}%`, t('summary.matureKept'))
          : stat(String(summary.hints), t('summary.hintsUsed', { count: summary.hints }))}
      </div>
      {summary.grammar > 0 && (
        <div className="bg-white rounded-xl px-3 py-2 text-sm text-center">
          {t('summary.wordsAndGrammar', { words: summary.words, grammar: summary.grammar })}
        </div>
      )}
      <div className="bg-white rounded-xl px-3 py-2 flex flex-col gap-1.5">
        <div className="text-[11px] text-gray-400">{t('summary.grades')}</div>
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
          <span>{t('summary.matureRetention')}</span>
          <span className={summary.mature_retention >= 90 ? 'text-green-600' : 'text-orange-500'}>
            {t('summary.retentionTarget', { n: summary.mature_retention })}
          </span>
        </div>
      )}
      {listCard(t('summary.mixedUp'), summary.mixed_up.map(m => ({ left: `${m.word} ↔ ${m.other}`, right: `${m.count}×`, tone: 'text-red-500' })))}
      {listCard(t('summary.readingSlips'), summary.reading_slips.map(s => ({ left: `${s.word} ${s.reading}`, right: t('summary.picked', { what: s.picked }), tone: 'text-orange-500' })))}
      {listCard(t('summary.contextMisses'), summary.context_misses.map(m => ({ left: m.word, right: t('summary.pickedMeaningOf', { word: m.other }), tone: 'text-red-500' })))}
    </div>
  )
}

const EMPTY_DETAIL: CardDetail = { reading: '', meaning: '', part_of_speech: '', example: '', example_meaning: '' }

const DETAIL_FIELDS: { key: keyof CardDetail; label: string; placeholder: string }[] = [
  { key: 'meaning', label: t('card.meaning'), placeholder: t('card.meaningPlaceholder') },
  { key: 'reading', label: t('card.reading'), placeholder: 'さんぽ' },
  { key: 'part_of_speech', label: t('card.partOfSpeech'), placeholder: t('pos.noun') },
  { key: 'example', label: t('card.example'), placeholder: '毎朝、公園を散歩します。' },
  { key: 'example_meaning', label: t('card.exampleMeaning'), placeholder: t('card.exampleMeaningPlaceholder') },
]

/**
 * A card with no meaning yet: generate or write the back, check it, then it is quizzed later.
 * A grammar card's back is AI-made practice material; only its meaning can be edited.
 */
export function MeaningSetup({ task, isGrammar = false, hasApi, generating, saving, error, onGenerate, onSave, onSkip, onOpenSettings }: {
  task: QuizTask
  isGrammar?: boolean
  hasApi: boolean
  generating: boolean
  saving: boolean
  error: string | null
  onGenerate: () => void
  onSave: (detail: CardDetail) => void
  onSkip: () => void
  onOpenSettings: () => void
}) {
  const [editing, setEditing] = useState<CardDetail | null>(null)
  const draft = task.draft

  const label = (
    <span className="text-[10px] font-bold uppercase tracking-wider px-2 py-0.5 rounded-full bg-orange-50 text-orange-500">
      {editing ? t('quiz.editBack') : draft ? t('quiz.checkBack') : isGrammar ? t('quiz.noPracticeYet') : t('quiz.noMeaningYet', { language: languageName(explainLang) })}
    </span>
  )

  if (editing) {
    return (
      <div className="flex flex-col gap-3 w-full max-w-sm mx-auto">
        <div className="bg-white rounded-2xl shadow-md px-5 py-5 flex flex-col gap-3">
          <div className="text-center flex flex-col items-center gap-1">
            {label}
            <div className="text-3xl font-semibold">{task.word}</div>
          </div>
          {DETAIL_FIELDS.filter(f => !isGrammar || f.key === 'meaning').map(f => (
            <label key={f.key} className="flex flex-col gap-1">
              <span className="text-xs text-gray-400">{f.label}{f.key === 'meaning' ? ` ${t('card.required')}` : ''}</span>
              <input
                id={`meaning-${f.key}`}
                value={editing[f.key]}
                placeholder={f.placeholder}
                onChange={e => setEditing({ ...editing, [f.key]: e.target.value })}
                className="px-3 py-2 rounded-xl bg-gray-50 text-sm outline-none focus:ring-2 focus:ring-blue-200"
              />
            </label>
          ))}
        </div>
        {error && <p className="text-xs text-center text-red-500">{error}</p>}
        <div className="grid grid-cols-2 gap-2">
          <button type="button" className={secondaryBtn} onClick={() => setEditing(null)}>{t('common.cancel')}</button>
          <button type="button" className={primaryBtn} disabled={saving || !editing.meaning.trim()} onClick={() => onSave(editing)}>
            {saving ? t('common.saving') : t('quiz.saveLater')}
          </button>
        </div>
      </div>
    )
  }

  if (generating) {
    return (
      <div className="flex flex-col gap-3 w-full max-w-sm mx-auto">
        <div className="bg-white rounded-2xl shadow-md px-6 py-8 text-center flex flex-col items-center gap-3">
          <div className="text-4xl font-semibold">{task.word}</div>
          <div className="w-full flex flex-col items-center gap-2 animate-pulse">
            <div className="h-2.5 w-2/5 rounded bg-gray-100" />
            <div className="h-2.5 w-3/4 rounded bg-gray-100" />
            <div className="h-2.5 w-4/5 rounded bg-gray-100" />
          </div>
          <span className="text-xs text-gray-400">{t('quiz.generatingBack')}</span>
        </div>
      </div>
    )
  }

  if (draft && isGrammar && task.grammar) {
    return (
      <div className="flex flex-col gap-3 w-full max-w-sm mx-auto">
        <div className="bg-white rounded-2xl shadow-md px-5 py-6 flex flex-col items-center gap-3">
          {label}
          <span className="text-[10px] font-semibold px-2 py-0.5 rounded-full bg-indigo-50 text-indigo-600">{t('common.aiMade')}</span>
          <GrammarInfoView title={task.word} info={task.grammar} />
        </div>
        {error && <p className="text-xs text-center text-red-500">{error}</p>}
        <button type="button" className={primaryBtn} disabled={saving} onClick={() => onSave(draft)}>
          {saving ? t('common.saving') : t('quiz.looksRight')}
        </button>
        <div className="grid grid-cols-2 gap-2">
          <button type="button" className={secondaryBtn} onClick={() => setEditing({ ...draft })}>✎ {t('quiz.editMeaning')}</button>
          <button type="button" className={secondaryBtn} disabled={!hasApi} onClick={onGenerate}>↻ {t('card.regenerate')}</button>
        </div>
        <button type="button" className="py-1 text-xs text-gray-400 hover:text-gray-600" onClick={onSkip}>{t('quiz.skipToday')}</button>
      </div>
    )
  }

  if (isGrammar) {
    return (
      <div className="flex flex-col gap-3 w-full max-w-sm mx-auto">
        <div className="bg-white rounded-2xl shadow-md px-6 py-8 text-center flex flex-col items-center gap-2">
          {label}
          <div className="text-3xl font-semibold">{task.word}</div>
          <span className="text-xs text-gray-400 mt-1">{hasApi ? t('quiz.practiceHint') : t('quiz.practiceNeedsApi')}</span>
        </div>
        {error && <p className="text-xs text-center text-red-500">{error}</p>}
        {hasApi ? (
          <button type="button" className={primaryBtn} onClick={onGenerate}>✨ {t('quiz.generatePractice')}</button>
        ) : (
          <button type="button" className={primaryBtn} onClick={onOpenSettings}>{t('quiz.openSettings')}</button>
        )}
        <button type="button" className={secondaryBtn} onClick={onSkip}>{t('quiz.skipToday')}</button>
      </div>
    )
  }

  if (draft) {
    return (
      <div className="flex flex-col gap-3 w-full max-w-sm mx-auto">
        <div className="bg-white rounded-2xl shadow-md px-5 py-6 flex flex-col gap-3">
          <div className="text-center flex flex-col items-center gap-1">
            {label}
            <Word word={task.word} reading={draft.reading} showReading size="text-3xl" />
            <span className="text-[10px] font-semibold px-2 py-0.5 rounded-full bg-indigo-50 text-indigo-600">{t('common.aiMade')}</span>
          </div>
          <div className="flex flex-col gap-1 text-sm">
            <div className="flex justify-between gap-3"><span className="text-gray-400">{t('card.meaning')}</span><span className="font-semibold text-right">{draft.meaning}</span></div>
            {draft.part_of_speech && (
              <div className="flex justify-between gap-3"><span className="text-gray-400">{t('card.partOfSpeech')}</span><span className="text-right">{partOfSpeechLabel(draft.part_of_speech)}</span></div>
            )}
          </div>
          {draft.example && (
            <div className="bg-gray-50 rounded-xl p-2.5 text-sm">
              <div>{draft.example}</div>
              {draft.example_meaning && <div className="text-xs text-gray-500 mt-0.5">{draft.example_meaning}</div>}
            </div>
          )}
        </div>
        {error && <p className="text-xs text-center text-red-500">{error}</p>}
        <button type="button" className={primaryBtn} disabled={saving} onClick={() => onSave(draft)}>
          {saving ? t('common.saving') : t('quiz.looksRight')}
        </button>
        <div className="grid grid-cols-2 gap-2">
          <button type="button" className={secondaryBtn} onClick={() => setEditing({ ...draft })}>✎ {t('common.edit')}</button>
          <button type="button" className={secondaryBtn} disabled={!hasApi} onClick={onGenerate}>↻ {t('card.regenerate')}</button>
        </div>
        <button type="button" className="py-1 text-xs text-gray-400 hover:text-gray-600" onClick={onSkip}>{t('quiz.skipToday')}</button>
      </div>
    )
  }

  return (
    <div className="flex flex-col gap-3 w-full max-w-sm mx-auto">
      <div className="bg-white rounded-2xl shadow-md px-6 py-8 text-center flex flex-col items-center gap-2">
        {label}
        <div className="text-4xl font-semibold">{task.word}</div>
        <span className="text-xs text-gray-400 mt-1">
          {hasApi ? t('quiz.addBackHint') : t('quiz.needApiHint')}
        </span>
      </div>
      {error && <p className="text-xs text-center text-red-500">{error}</p>}
      {hasApi ? (
        <button type="button" className={primaryBtn} onClick={onGenerate}>✨ {t('quiz.generateMeaning')}</button>
      ) : (
        <button type="button" className={primaryBtn} onClick={() => setEditing({ ...EMPTY_DETAIL })}>{t('quiz.writeMyself')}</button>
      )}
      <div className="grid grid-cols-2 gap-2">
        {hasApi ? (
          <button type="button" className={secondaryBtn} onClick={() => setEditing({ ...EMPTY_DETAIL })}>{t('quiz.writeMyself')}</button>
        ) : (
          <button type="button" className={secondaryBtn} onClick={onOpenSettings}>{t('quiz.openSettings')}</button>
        )}
        <button type="button" className={secondaryBtn} onClick={onSkip}>{t('quiz.skipToday')}</button>
      </div>
    </div>
  )
}
