import { Fragment, useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { useNavigate } from 'react-router-dom'
import { invoke } from '@tauri-apps/api/core'
import {
  ArrowLeftOutlined, CaretRightFilled, CheckOutlined, CloseOutlined, FlagFilled, LeftOutlined, LoadingOutlined, RightOutlined,
} from '@ant-design/icons'
import type { JlptOverview, JlptPart, JlptResult, JlptSection, JlptSessionView } from '../lib/types'
import { t, errorText } from '../lib/i18n'
import { CountUp, burstConfetti, reducedMotion } from '../components/Motion'
import { JLPT_LEVEL_ORDER, JLPT_PRACTICE_LEVEL_KEY, PART_INSTRUCTION, SECTION_JA, SPOKEN_CHOICE_PARTS, practiceLevel, type JLPTLevel } from '../lib/jlpt'

type Phase = 'hub' | 'brief' | 'exam' | 'result'


function clock(seconds: number): string {
  const s = Math.max(0, Math.round(seconds))
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`
}

/** Question text with the booklet's marks: <u>underlined</u> words, line breaks, 【1】 gaps and the ★ blank. */
function Marked({ text, gap }: { text: string; gap?: string }) {
  const pieces = text.split(/(<u>.*?<\/u>|<br\s*\/?>|\n|【\d+】|★)/g)
  return (
    <>
      {pieces.map((p, i) => {
        if (!p) return null
        if (p.startsWith('<u>')) return <span key={i} className="underline decoration-[1.5px] underline-offset-[6px]">{p.slice(3, -4)}</span>
        if (/^<br/.test(p) || p === '\n') return <br key={i} />
        if (/^【\d+】$/.test(p)) {
          const cur = p === gap
          return (
            <span key={i} className={`inline-block mx-0.5 px-1.5 leading-snug border-[1.5px] font-sans font-bold text-[13px] ${cur ? 'border-blue-500 bg-blue-50 text-blue-600' : 'border-gray-800'}`}>
              {p.slice(1, -1)}
            </span>
          )
        }
        if (p === '★') return <span key={i} className="text-blue-600 font-bold">★</span>
        return <Fragment key={i}>{p}</Fragment>
      })}
    </>
  )
}

/** Plays a recording once with the device's Japanese voice. Returns false when the device has no speech. */
function speak(lines: string[], onEnd: () => void): boolean {
  if (!('speechSynthesis' in window) || typeof SpeechSynthesisUtterance === 'undefined') return false
  window.speechSynthesis.cancel()
  const text = lines.map((l) => l.replace(/^(男|女|[A-Z])[：:]/, '')).join('。 ')
  const u = new SpeechSynthesisUtterance(text)
  u.lang = 'ja-JP'
  u.rate = 0.9
  const voice = window.speechSynthesis.getVoices().find((v) => v.lang?.startsWith('ja'))
  if (voice) u.voice = voice
  u.onend = onEnd
  u.onerror = onEnd
  window.speechSynthesis.speak(u)
  return true
}

function stopSpeech() {
  try { window.speechSynthesis?.cancel() } catch { /* no speech */ }
}

/** Full-screen call: the 3-2-1 before the start, はじめてください, 時間です. */
function Call({ big, sub, count, shake }: { big: string; sub?: string; count?: boolean; shake?: boolean }) {
  return (
    <div className="fixed inset-0 z-[60] bg-gray-50 flex items-center justify-center text-center p-6 anim-fade">
      <div className="flex flex-col items-center gap-2">
        <div key={big} className={`${count ? 'text-7xl font-bold text-blue-500 tabular-nums' : 'font-serif text-4xl font-semibold'} anim-stamp ${shake ? 'anim-stamp-shake' : ''}`}>{big}</div>
        {sub && <div className="text-sm text-gray-500">{sub}</div>}
      </div>
    </div>
  )
}

function Sheet({ children, onClose }: { children: ReactNode; onClose: () => void }) {
  return (
    <div className="fixed inset-0 z-[55] bg-black/40 flex items-end justify-center anim-fade" onClick={onClose}>
      <div className="w-full max-w-lg bg-white rounded-t-3xl p-5 pb-8 safe-bottom flex flex-col gap-4 anim-sheet" onClick={(e) => e.stopPropagation()}>
        {children}
      </div>
    </div>
  )
}

const btn = 'py-3 px-4 rounded-2xl text-sm font-medium transition-transform active:scale-95 disabled:opacity-50'
const primaryBtn = `${btn} bg-blue-500 text-white hover:bg-blue-600 shadow-sm`
const secondaryBtn = `${btn} bg-gray-100 text-gray-700 hover:bg-gray-200`

export default function JlptPractice() {
  const navigate = useNavigate()
  const [level, setLevel] = useState<JLPTLevel>(practiceLevel)
  const [phase, setPhase] = useState<Phase>('hub')
  /** Slide direction for the screen being shown. */
  const [enter, setEnter] = useState<'fwd' | 'back'>('fwd')
  const [overview, setOverview] = useState<JlptOverview | null>(null)
  const [section, setSection] = useState<JlptSection>('vocab')
  const [session, setSession] = useState<JlptSessionView | null>(null)
  const [preparing, setPreparing] = useState(false)
  const [error, setError] = useState('')
  const [answers, setAnswers] = useState<(number | null)[]>([])
  const [flags, setFlags] = useState<boolean[]>([])
  const [index, setIndex] = useState(0)
  const [qDir, setQDir] = useState<'next' | 'prev' | ''>('')
  const [justPicked, setJustPicked] = useState(false)
  const [left, setLeft] = useState(0)
  const [sheet, setSheet] = useState<'' | 'grid' | 'finish' | 'quit'>('')
  const [call, setCall] = useState<{ big: string; sub?: string; count?: boolean; shake?: boolean } | null>(null)
  const [played, setPlayed] = useState<Record<number, 'playing' | 'done'>>({})
  const [scriptShown, setScriptShown] = useState<Record<number, boolean>>({})
  const [result, setResult] = useState<JlptResult | null>(null)
  const [justDone, setJustDone] = useState<JlptSection | null>(null)
  const [cardsAdded, setCardsAdded] = useState(false)
  const [showGloss, setShowGloss] = useState(false)
  const submitting = useRef(false)
  /** Latest answers, for the clock's hand-in when time runs out (its callback outlives the render it came from). */
  const answersRef = useRef<(number | null)[]>([])
  answersRef.current = answers
  const prepareFor = useRef('')

  const apiHost = localStorage.getItem('api_host') || ''
  const apiKey = localStorage.getItem('api_key') || ''
  const model = localStorage.getItem('api_model') || ''

  const plan = overview?.plans.find((p) => p.section === section)

  /** Every question in order with the part it belongs to. */
  const flat = useMemo(() => {
    const out: { part: JlptPart; partNo: number; q: number }[] = []
    session?.parts.forEach((part, partNo) => part.questions.forEach((_, q) => out.push({ part, partNo, q })))
    return out
  }, [session])

  async function loadOverview(lv = level) {
    try {
      setOverview(await invoke<JlptOverview>('db_jlpt_overview', { level: lv }))
    } catch (e) {
      setError(errorText(e))
    }
  }

  useEffect(() => { loadOverview() }, [level])
  useEffect(() => () => stopSpeech(), [])

  function go(next: Phase, dir: 'fwd' | 'back' = 'fwd') {
    setEnter(dir)
    setPhase(next)
    window.scrollTo(0, 0)
    document.querySelector('main > div')?.scrollTo?.(0, 0)
  }

  function pickLevel(lv: JLPTLevel) {
    setLevel(lv)
    try { localStorage.setItem(JLPT_PRACTICE_LEVEL_KEY, lv) } catch { /* storage blocked */ }
  }

  /** Opens a section's briefing and starts writing its questions, so they're ready by the time the learner taps Begin. */
  async function openBrief(s: JlptSection) {
    setSection(s)
    setSession(null)
    setError('')
    go('brief')
    if (!apiKey) { setError(t('common.apiMissing')); return }
    const key = `${level}:${s}:${Date.now()}`
    prepareFor.current = key
    setPreparing(true)
    try {
      const view = await invoke<JlptSessionView>('llm_jlpt_prepare', { apiHost, apiKey, model, level, section: s })
      if (prepareFor.current === key) setSession(view)
    } catch (e) {
      if (prepareFor.current === key) setError(errorText(e))
    } finally {
      if (prepareFor.current === key) setPreparing(false)
    }
  }

  function startExam(view: JlptSessionView) {
    const total = view.parts.reduce((n, p) => n + p.questions.length, 0)
    setAnswers(view.answers.length === total ? view.answers : Array(total).fill(null))
    setFlags(Array(total).fill(false))
    setIndex(0)
    setPlayed({})
    setScriptShown({})
    setSheet('')
    setSession(view)
    submitting.current = false
    go('exam')
  }

  async function begin() {
    if (!session) return
    const sub = `${SECTION_JA[session.section]} · ${clock(session.time_limit)}`
    const counts = reducedMotion() ? [] : ['3', '2', '1']
    for (const c of counts) {
      setCall({ big: c, sub, count: true })
      await new Promise((r) => setTimeout(r, 650))
    }
    setCall({ big: t('jlpt.callStart'), sub })
    try {
      const view = await invoke<JlptSessionView>('db_jlpt_begin', { id: session.id })
      await new Promise((r) => setTimeout(r, 1100))
      startExam(view)
    } catch (e) {
      setError(errorText(e))
    } finally {
      setCall(null)
    }
  }

  async function resume(id: number) {
    try {
      const view = await invoke<JlptSessionView>('db_jlpt_get_session', { id })
      setSection(view.section)
      startExam(view)
    } catch (e) {
      setError(errorText(e))
    }
  }

  async function submit(timedOut: boolean) {
    if (!session || submitting.current) return
    submitting.current = true
    stopSpeech()
    setSheet('')
    if (timedOut) {
      setCall({ big: t('jlpt.callTimeUp'), sub: t('jlpt.callTimeUpSub'), shake: true })
      await new Promise((r) => setTimeout(r, 1500))
    }
    try {
      const r = await invoke<JlptResult>('db_jlpt_submit', { id: session.id, answers: answersRef.current, timedOut })
      setResult(r)
      setCardsAdded(false)
      setJustDone(overview?.next_section === r.section ? r.section : null)
      go('result')
      if (r.total > 0 && r.correct / r.total >= 0.7) setTimeout(burstConfetti, 900)
      loadOverview()
    } catch (e) {
      submitting.current = false
      setError(errorText(e))
    } finally {
      setCall(null)
    }
  }

  // The clock: counts from the stored start time, so it keeps running while the app is in the background.
  useEffect(() => {
    if (phase !== 'exam' || !session?.started_at) return
    const deadline = session.started_at + session.time_limit * 1000
    const tick = () => {
      const remaining = Math.ceil((deadline - Date.now()) / 1000)
      setLeft(Math.max(0, remaining))
      if (remaining <= 0) submit(true)
    }
    tick()
    const timer = setInterval(tick, 500)
    return () => clearInterval(timer)
  }, [phase, session])

  function pick(k: number) {
    const next = answers.slice()
    next[index] = k
    setAnswers(next)
    setJustPicked(true)
    if (session) invoke('db_jlpt_save_answers', { id: session.id, answers: next }).catch(() => {})
  }

  function move(to: number) {
    if (to < 0 || to >= flat.length || to === index) return
    stopSpeech()
    setPlayed((p) => (p[index] === 'playing' ? { ...p, [index]: 'done' } : p))
    setQDir(to > index ? 'next' : 'prev')
    setJustPicked(false)
    setIndex(to)
    setSheet('')
  }

  function play() {
    const cur = flat[index]
    if (!cur) return
    const q = cur.part.questions[cur.q]
    const lines = SPOKEN_CHOICE_PARTS.includes(cur.part.part)
      ? [...q.script, ...q.options.map((o, i) => `${['いち', 'に', 'さん'][i] ?? i + 1}。${o}`)]
      : q.script
    const i = index
    const ok = speak(lines, () => setPlayed((p) => ({ ...p, [i]: 'done' })))
    if (ok) setPlayed((p) => ({ ...p, [i]: 'playing' }))
    else {
      setPlayed((p) => ({ ...p, [i]: 'done' }))
      setScriptShown((s) => ({ ...s, [i]: true }))
      setError(t('jlpt.noVoice'))
    }
  }

  async function addMissedWords() {
    if (!result) return
    for (const word of result.missed_words) {
      await invoke('db_upsert_review_item', { content: word, language: 'ja' }).catch(() => {})
    }
    setCardsAdded(true)
  }

  const enterClass = enter === 'fwd' ? 'anim-enter-fwd' : 'anim-enter-back'

  // ---------------------------------------------------------------- hub
  function renderHub() {
    const o = overview
    const next = o?.plans.find((p) => p.section === o.next_section)
    return (
      <div key="hub" className={`flex flex-col gap-5 pb-6 ${enterClass}`}>
        <div className="sticky top-0 z-10 pt-4 pb-2 bg-gray-50 flex items-center justify-between gap-2">
          <button onClick={() => navigate('/')} className="flex items-center gap-1 text-gray-500 text-sm">
            <ArrowLeftOutlined /> {t('nav.home')}
          </button>
          <div className="flex gap-1">
            {JLPT_LEVEL_ORDER.map((lv) => (
              <button
                key={lv}
                onClick={() => pickLevel(lv)}
                className={`px-2.5 py-1 rounded-lg text-xs font-bold border-[1.5px] transition-colors ${lv === level ? 'border-blue-500 bg-blue-50 text-blue-600' : 'border-gray-200 bg-white text-gray-500'}`}
              >
                {lv}
              </button>
            ))}
          </div>
        </div>
        <h2 className="text-xl font-bold -mt-3 anim-stag">{t('jlpt.title')}</h2>

        {o?.open_session && (
          <div className="bg-amber-50 border border-amber-100 rounded-xl p-4 flex items-center gap-3 anim-stag">
            <div className="flex-1 text-sm text-amber-800">{t('jlpt.openSession')}</div>
            <button className={`${primaryBtn} py-2`} onClick={() => resume(o.open_session!)}>{t('jlpt.resume')}</button>
          </div>
        )}

        {o && next && (
          <div className="anim-hero rounded-2xl p-5 text-white flex flex-col gap-3 anim-stag" style={{ ['--i' as string]: 1 }}>
            <div className="text-xs uppercase tracking-wider text-blue-100">
              {o.done_today ? t('jlpt.doneTodayNext') : t('jlpt.todaysSession')} · {t('jlpt.rotation', { n: o.cycle })}
            </div>
            <div className="text-2xl font-bold">{SECTION_JA[next.section]} · {t(`jlpt.section.${next.section}`)}</div>
            <div className="flex flex-wrap gap-1.5 text-xs tabular-nums">
              <span className="bg-white/20 rounded-full px-2.5 py-0.5">{t('jlpt.questions', { count: next.questions })}</span>
              <span className="bg-white/20 rounded-full px-2.5 py-0.5">{t('jlpt.limit', { time: clock(next.time_limit) })}</span>
              <span className="bg-white/20 rounded-full px-2.5 py-0.5">{t('jlpt.realMinutes', { n: next.real_minutes })}</span>
            </div>
            <button onClick={() => openBrief(next.section)} className={`${btn} bg-white text-blue-700 font-bold relative overflow-hidden anim-shine`}>
              {o.done_today ? t('jlpt.practiceMore') : t('jlpt.startSession')}
            </button>
          </div>
        )}

        {o && (
          <div className="flex flex-col gap-2 anim-stag" style={{ ['--i' as string]: 2 }}>
            <h3 className="text-sm font-medium text-gray-500 uppercase tracking-wide px-1">{t('jlpt.rotationTitle')}</h3>
            <div className="grid gap-1.5" style={{ gridTemplateColumns: `repeat(${o.rotation.length}, minmax(0, 1fr))` }}>
              {o.rotation.map((step, i) => (
                <button
                  key={i}
                  onClick={() => openBrief(step.section)}
                  className={`rounded-xl border-[1.5px] py-2.5 px-1 flex flex-col items-center gap-0.5 transition-transform active:scale-95
                    ${step.state === 'next' ? 'border-blue-500 bg-blue-50 anim-halo' : 'border-gray-200 bg-white'}`}
                >
                  <span className="text-[13px] font-bold">{SECTION_JA[step.section]}</span>
                  <span className="text-[10.5px] text-gray-400 truncate max-w-full">{t(`jlpt.section.${step.section}`)}</span>
                  <span className={`text-[10.5px] tabular-nums ${step.state === 'next' ? 'text-blue-600 font-medium' : step.state === 'done' ? 'text-green-600' : 'text-gray-400'} ${justDone === step.section && step.state === 'done' ? 'anim-pop' : ''}`}>
                    {step.state === 'next' ? t('jlpt.next') : step.last ? `${step.state === 'done' ? '✓ ' : ''}${step.last.correct}/${step.last.total}` : '—'}
                  </span>
                </button>
              ))}
            </div>
            <p className="text-xs text-gray-400 px-1">{t('jlpt.rotationHint')}</p>
          </div>
        )}

        {o && (
          <div className="bg-white rounded-xl p-4 shadow-sm flex flex-col gap-3 anim-stag" style={{ ['--i' as string]: 3 }}>
            <h3 className="text-sm font-medium text-gray-500 uppercase tracking-wide">{t('jlpt.estimateTitle')}</h3>
            {o.estimate ? (
              <>
                {o.estimate.parts.map((p) => (
                  <div key={p.code} className="flex flex-col gap-1">
                    <div className="flex justify-between text-sm">
                      <span>{t(`jlpt.scorePart.${p.code}`)}</span>
                      <span className="font-bold tabular-nums"><CountUp value={p.score} /><span className="text-gray-400 font-normal text-xs">/{p.max}</span></span>
                    </div>
                    <div className="relative h-2 rounded-full bg-gray-100">
                      <div className="absolute inset-y-0 left-0 rounded-full bg-blue-500 anim-grow" style={{ width: `${(p.score / p.max) * 100}%` }} />
                      <div className="absolute -top-1 -bottom-1 w-0.5 bg-gray-700/60" style={{ left: `${(p.min / p.max) * 100}%` }} title={t('jlpt.minimum', { n: p.min })} />
                    </div>
                  </div>
                ))}
                <div className="flex items-center justify-between border-t border-gray-100 pt-3">
                  <span className="text-sm">{t('jlpt.total')}</span>
                  <span className="flex items-center gap-2">
                    <span className="text-xl font-bold tabular-nums"><CountUp value={o.estimate.total} /></span>
                    <span className="text-xs text-gray-400">{t('jlpt.passMark', { max: o.estimate.max, pass: o.estimate.pass_mark })}</span>
                    <span className={`text-xs px-2 py-0.5 rounded-full font-medium ${o.estimate.passed ? 'bg-green-50 text-green-600' : 'bg-red-50 text-red-500'}`}>
                      {o.estimate.passed ? t('jlpt.onTrack') : t('jlpt.below')}
                    </span>
                  </span>
                </div>
                <p className="text-xs text-gray-400">{t('jlpt.estimateNote')}</p>
              </>
            ) : (
              <p className="text-sm text-gray-500">{t('jlpt.estimateEmpty')}</p>
            )}
          </div>
        )}

        {o && (
          <div className="bg-white rounded-xl p-4 shadow-sm flex flex-col gap-3 anim-stag" style={{ ['--i' as string]: 4 }}>
            <h3 className="text-sm font-medium text-gray-500 uppercase tracking-wide">{t('jlpt.week')}</h3>
            <div className="grid grid-cols-7 gap-1.5">
              {o.week.map((on, i) => (
                <div
                  key={i}
                  className={`aspect-square rounded-lg flex items-center justify-center text-xs ${on ? 'bg-green-50 text-green-600 font-bold anim-pop' : 'bg-gray-100'} ${i === 6 ? 'ring-2 ring-blue-400 ring-inset' : ''}`}
                  style={{ animationDelay: `${300 + i * 60}ms` }}
                >
                  {on ? <CheckOutlined /> : null}
                </div>
              ))}
            </div>
            <p className="text-xs text-gray-400">{t('jlpt.weekHint')}</p>
          </div>
        )}

        {error && <p className="text-sm text-red-500">{error}</p>}
      </div>
    )
  }

  // ---------------------------------------------------------------- briefing
  function renderBrief() {
    return (
      <div key="brief" className={`flex flex-col gap-4 pb-6 min-h-full ${enterClass}`}>
        <div className="sticky top-0 z-10 pt-4 pb-2 bg-gray-50 flex items-center justify-between">
          <button onClick={() => { prepareFor.current = ''; setPreparing(false); go('hub', 'back') }} className="flex items-center gap-1 text-gray-500 text-sm">
            <ArrowLeftOutlined /> {t('common.back')}
          </button>
          <span className="text-xs px-2.5 py-1 rounded-full bg-blue-50 text-blue-600 font-medium">{level} · {t(`jlpt.section.${section}`)}</span>
        </div>
        <div className="bg-white border-[1.5px] border-gray-800 rounded p-5 font-serif flex flex-col gap-3 anim-stag">
          <div className="text-4xl font-semibold tracking-wide">{level}</div>
          <div className="text-xl font-semibold">{SECTION_JA[section]}</div>
          {plan && (
            <>
              <div className="flex flex-wrap gap-1 font-sans">
                {plan.parts.map((p, i) => (
                  <span key={i} className="text-[11px] px-2 py-0.5 rounded-full bg-gray-100 text-gray-600">{t(`jlpt.part.${p.part}`)} × {p.count}</span>
                ))}
              </div>
              <div className="border-t border-gray-100 pt-3 font-sans text-sm">
                <b>{t('jlpt.questions', { count: plan.questions })} · {clock(plan.time_limit)}</b>
                <span className="text-gray-500"> · {t('jlpt.pace', { n: Math.round(plan.time_limit / plan.questions) })}</span>
                <div className="text-xs text-gray-400 mt-1">
                  {plan.real_block ? t(`jlpt.realBlock.${plan.real_block}`, { n: plan.real_minutes }) : t('jlpt.realSection', { n: plan.real_minutes })}
                </div>
              </div>
            </>
          )}
        </div>
        <ul className="list-disc pl-5 text-sm flex flex-col gap-1.5 text-gray-700 anim-stag" style={{ ['--i' as string]: 1 }}>
          <li>{t('jlpt.rule.clock')}</li>
          <li>{t('jlpt.rule.skip')}</li>
          <li>{t('jlpt.rule.noFeedback')}</li>
          {section === 'listening' && <li>{t('jlpt.rule.listening')}</li>}
          <li>{t('jlpt.rule.leave')}</li>
        </ul>
        <label className="bg-white rounded-xl p-4 shadow-sm flex items-center justify-between gap-3 anim-stag" style={{ ['--i' as string]: 2 }}>
          <span>
            <span className="block text-sm font-medium">{t('jlpt.gloss')}</span>
            <span className="block text-xs text-gray-400">{t('jlpt.glossHint')}</span>
          </span>
          <input type="checkbox" checked={showGloss} onChange={(e) => setShowGloss(e.target.checked)} className="w-5 h-5 accent-blue-500" />
        </label>
        {error && <p className="text-sm text-red-500">{error}</p>}
        <div className="mt-auto flex flex-col gap-2">
          {error && !preparing && !session && apiKey && (
            <button className={secondaryBtn} onClick={() => openBrief(section)}>{t('common.retry')}</button>
          )}
          <button className={primaryBtn} disabled={!session || preparing} onClick={begin}>
            {preparing ? <><LoadingOutlined className="mr-2" />{t('jlpt.preparing')}</> : t('jlpt.begin', { time: plan ? clock(plan.time_limit) : '' })}
          </button>
        </div>
      </div>
    )
  }

  // ---------------------------------------------------------------- exam
  function renderExam() {
    const cur = flat[index]
    if (!session || !cur) return null
    const q = cur.part.questions[cur.q]
    const spoken = SPOKEN_CHOICE_PARTS.includes(cur.part.part)
    const listening = q.script.length > 0
    const answered = answers.filter((a) => a !== null).length
    const warn = left <= 30 ? 'red' : left <= 60 ? 'amber' : ''
    const gap = q.stem.match(/【\d+】/)?.[0]
    const play_ = played[index]
    return (
      <div key="exam" className={`flex flex-col min-h-full ${enterClass}`}>
        <div className="sticky top-0 z-10 bg-gray-50 pt-3 pb-2 flex flex-col gap-2">
          <div className="flex items-center justify-between gap-2">
            <button onClick={() => setSheet('quit')} className="w-9 h-9 flex items-center justify-center text-gray-500" aria-label={t('jlpt.quit')}><CloseOutlined /></button>
            <span className="text-xs text-gray-500">{SECTION_JA[session.section]} · {index + 1} / {flat.length}</span>
            <span
              key={left <= 10 ? left : 'steady'}
              className={`tabular-nums font-bold text-base px-3 py-1 rounded-full border-[1.5px] transition-colors
                ${warn === 'red' ? 'text-red-600 border-red-500 bg-red-50 anim-pulse' : warn === 'amber' ? 'text-amber-600 border-amber-500 bg-amber-50' : 'bg-white border-gray-200'}
                ${left <= 10 && left > 0 ? 'anim-shake' : ''}`}
            >
              {clock(left)}
            </span>
          </div>
          <div className="h-1 rounded-full bg-gray-200 overflow-hidden">
            <div
              className={`h-full transition-[width] duration-500 ease-linear ${warn === 'red' ? 'bg-red-500' : warn === 'amber' ? 'bg-amber-500' : 'bg-blue-500'}`}
              style={{ width: `${(left / session.time_limit) * 100}%` }}
            />
          </div>
        </div>

        <div key={index} className={`flex flex-col gap-4 pt-2 pb-4 ${qDir === 'next' ? 'anim-enter-fwd' : qDir === 'prev' ? 'anim-enter-back' : ''}`}>
          <div className="flex flex-col gap-1">
            <div className="flex gap-2 items-baseline">
              <span className="font-serif font-semibold border-[1.5px] border-gray-800 px-2 text-sm shrink-0">{`問題${cur.partNo + 1}`}</span>
              <span className="font-serif text-sm leading-relaxed">{PART_INSTRUCTION[cur.part.part] ?? ''}</span>
            </div>
            {showGloss && <span className="text-xs text-gray-400">{t(`jlpt.part.${cur.part.part}`)} — {t(`jlpt.partHint.${cur.part.part}`)}</span>}
          </div>

          {cur.part.passage && (
            <div className="bg-white border border-gray-200 rounded-md p-4 font-serif text-[15px] leading-loose whitespace-pre-wrap">
              <Marked text={cur.part.passage} gap={gap} />
            </div>
          )}

          {listening && (
            <div className="bg-white rounded-xl p-4 shadow-sm flex flex-col gap-2">
              <div className="flex items-center gap-3">
                <button
                  onClick={play}
                  disabled={!!play_}
                  className={`w-12 h-12 rounded-full flex items-center justify-center text-lg shrink-0 ${play_ ? 'bg-gray-200 text-gray-400' : 'bg-blue-500 text-white anim-halo'}`}
                  aria-label={t('jlpt.play')}
                >
                  {play_ === 'playing' ? <LoadingOutlined /> : <CaretRightFilled />}
                </button>
                <div>
                  <div className="text-sm font-medium">{play_ === 'playing' ? t('jlpt.playing') : play_ ? t('jlpt.played') : t('jlpt.play')}</div>
                  <div className="text-xs text-gray-400">{t('jlpt.playOnce')}</div>
                </div>
              </div>
              {scriptShown[index] && <div className="font-serif text-sm bg-gray-50 rounded-lg p-3 leading-loose">{q.script.map((l, i) => <div key={i}>{l}</div>)}</div>}
            </div>
          )}

          <div className="flex flex-col gap-1.5">
            <span className="text-xs font-bold text-gray-400">{gap ? <Marked text={gap} gap={gap} /> : index + 1}</span>
            {q.stem && !gap && <div className="font-serif text-lg leading-loose"><Marked text={q.stem} /></div>}
          </div>

          <div className="flex flex-col gap-2">
            {q.options.map((o, k) => {
              const on = answers[index] === k
              return (
                <button
                  key={k}
                  onClick={() => pick(k)}
                  className={`flex items-center gap-3 text-left w-full bg-white rounded-xl border-[1.5px] px-3 py-2.5 transition-colors
                    ${on ? 'border-gray-900' : 'border-gray-200 hover:border-blue-300'} ${on && justPicked ? 'anim-press' : ''}`}
                >
                  <span className={`relative w-7 h-7 rounded-full border-[1.5px] flex items-center justify-center text-xs font-bold shrink-0
                    ${on ? 'bg-gray-900 border-gray-900 text-white' : 'border-gray-400 text-gray-500'} ${on && justPicked ? 'anim-ink' : ''}`}>
                    {k + 1}
                  </span>
                  <span className={spoken ? 'text-sm text-gray-400' : 'font-serif text-base'}>{spoken ? t('jlpt.spokenChoice', { n: k + 1 }) : <Marked text={o} />}</span>
                </button>
              )
            })}
          </div>
          {error && <p className="text-sm text-red-500">{error}</p>}
        </div>

        <div className="sticky bottom-0 mt-auto bg-gray-50 border-t border-gray-200 py-2.5 grid grid-cols-[auto_auto_1fr_auto] gap-2 items-center">
          <button className={secondaryBtn} disabled={index === 0} onClick={() => move(index - 1)} aria-label={t('common.back')}><LeftOutlined /></button>
          <button
            className={`${btn} ${flags[index] ? 'bg-amber-50 text-amber-600' : 'bg-gray-100 text-gray-500'}`}
            onClick={() => setFlags((f) => f.map((x, i) => (i === index ? !x : x)))}
            aria-label={t('jlpt.flag')}
          >
            <FlagFilled />
          </button>
          <button className={secondaryBtn} onClick={() => setSheet('grid')}>{t('jlpt.answerSheet', { n: answered, total: flat.length })}</button>
          {index === flat.length - 1
            ? <button className={primaryBtn} onClick={() => (answers.some((a) => a === null) ? setSheet('finish') : submit(false))}>{t('jlpt.handIn')}</button>
            : <button className={primaryBtn} onClick={() => move(index + 1)} aria-label={t('common.next')}><RightOutlined /></button>}
        </div>

        {sheet === 'grid' && (
          <Sheet onClose={() => setSheet('')}>
            <div className="flex justify-between items-center">
              <b>{t('jlpt.answerSheetTitle')}</b>
              <span className="text-xs text-gray-400">{t('jlpt.answerSheetLegend')}</span>
            </div>
            <div className="grid grid-cols-5 gap-2">
              {flat.map((_, k) => (
                <button
                  key={k}
                  onClick={() => move(k)}
                  className={`relative rounded-xl border-[1.5px] py-2 text-sm font-bold tabular-nums transition-transform active:scale-95
                    ${answers[k] !== null ? 'bg-gray-900 border-gray-900 text-white' : 'bg-white border-gray-200'} ${k === index ? 'ring-2 ring-blue-400 ring-offset-1' : ''}`}
                >
                  {k + 1}
                  {flags[k] && <span className="absolute top-1 right-1 w-2 h-2 rounded-full bg-amber-500" />}
                </button>
              ))}
            </div>
            <button className={primaryBtn} onClick={() => (answers.some((a) => a === null) ? setSheet('finish') : submit(false))}>{t('jlpt.handIn')}</button>
          </Sheet>
        )}
        {sheet === 'finish' && (
          <Sheet onClose={() => setSheet('')}>
            <b>{t('jlpt.finishTitle', { count: answers.filter((a) => a === null).length })}</b>
            <p className="text-sm text-gray-500">{t('jlpt.finishBody', { time: clock(left) })}</p>
            <div className="grid grid-cols-2 gap-2">
              <button className={secondaryBtn} onClick={() => setSheet('')}>{t('jlpt.keepGoing')}</button>
              <button className={primaryBtn} onClick={() => submit(false)}>{t('jlpt.handIn')}</button>
            </div>
          </Sheet>
        )}
        {sheet === 'quit' && (
          <Sheet onClose={() => setSheet('')}>
            <b>{t('jlpt.quitTitle')}</b>
            <p className="text-sm text-gray-500">{t('jlpt.quitBody')}</p>
            <div className="grid grid-cols-2 gap-2">
              <button className={secondaryBtn} onClick={() => setSheet('')}>{t('jlpt.keepGoing')}</button>
              <button className={primaryBtn} onClick={() => submit(false)}>{t('jlpt.endAndScore')}</button>
            </div>
          </Sheet>
        )}
      </div>
    )
  }

  // ---------------------------------------------------------------- result
  function renderResult() {
    const r = result
    if (!r) return null
    const pct = r.total ? r.correct / r.total : 0
    const circumference = 2 * Math.PI * 50
    const seal = pct >= 0.9 ? 'たいへん\nよく\nできました' : pct >= 0.7 ? 'よく\nできました' : 'がんばり\nましょう'
    const nextPlan = overview?.plans.find((p) => p.section === r.next_section)
    let n = 0
    return (
      <div key={`result-${r.id}`} className={`flex flex-col gap-4 pb-6 ${enterClass}`}>
        <div className="pt-4 flex items-center justify-between">
          <h2 className="text-xl font-bold">{r.timed_out ? t('jlpt.callTimeUp') : t('jlpt.done')}</h2>
          <span className="text-xs px-2.5 py-1 rounded-full bg-blue-50 text-blue-600 font-medium">{r.level} · {t(`jlpt.section.${r.section}`)}</span>
        </div>

        <div className="relative bg-white rounded-2xl shadow-sm p-4 pr-24 flex items-center gap-4 anim-stag">
          <svg viewBox="0 0 120 120" className="w-28 h-28 shrink-0" role="img" aria-label={t('jlpt.scoreAria', { correct: r.correct, total: r.total })}>
            <circle cx="60" cy="60" r="50" fill="none" stroke="#f3f4f6" strokeWidth="10" />
            <circle
              cx="60" cy="60" r="50" fill="none" stroke="#3b82f6" strokeWidth="10" strokeLinecap="round"
              className="anim-ring" transform="rotate(-90 60 60)"
              style={{ strokeDasharray: `${circumference * pct} ${circumference}`, ['--ring-len' as string]: `${circumference}` }}
            />
            <text x="60" y="64" textAnchor="middle" fontSize="26" fontWeight="700" fill="#111827"><CountUp value={r.correct} />/{r.total}</text>
          </svg>
          <div className="flex flex-col gap-1 min-w-0 text-sm">
            <b>{SECTION_JA[r.section]} · {t('jlpt.rotation', { n: r.cycle })}</b>
            <span className="text-gray-500 text-xs">{r.timed_out ? t('jlpt.timedOutNote') : t('jlpt.handedInWith', { time: clock(r.time_limit - r.time_used) })}</span>
            <span className="text-xs">{t('jlpt.lastTime', { score: r.prev ? `${r.prev.correct}/${r.prev.total}` : '—' })}</span>
          </div>
          <div className="anim-seal absolute top-3 right-3 w-[76px] h-[76px] rounded-full border-[2.5px] border-red-600 text-red-600 flex items-center justify-center text-center font-serif font-semibold text-[11px] leading-tight whitespace-pre-line p-1.5" aria-hidden="true">
            {seal}
          </div>
        </div>

        <div className="grid grid-cols-3 gap-2 anim-stag" style={{ ['--i' as string]: 1 }}>
          {[
            [clock(r.time_used), t('jlpt.timeUsed', { time: clock(r.time_limit) })],
            [`${r.answered ? Math.round(r.time_used / r.answered) : 0}s`, t('jlpt.perQuestion', { n: Math.round(r.time_limit / Math.max(1, r.total)) })],
            [String(r.total - r.answered), t('jlpt.leftBlank')],
          ].map(([v, l], i) => (
            <div key={i} className="bg-gray-100 rounded-xl p-2.5 flex flex-col min-w-0">
              <b className="tabular-nums text-base">{v}</b>
              <span className="text-[11px] text-gray-500">{l}</span>
            </div>
          ))}
        </div>

        <div className="bg-white rounded-xl shadow-sm p-4 flex flex-col gap-2.5 anim-stag" style={{ ['--i' as string]: 2 }}>
          <h3 className="text-sm font-medium text-gray-500 uppercase tracking-wide">{t('jlpt.byPart')}</h3>
          {r.parts.map((p, i) => (
            <div key={i} className="flex flex-col gap-1 text-sm">
              <div className="flex justify-between"><span>{`問題${i + 1}`} <span className="text-gray-400">{t(`jlpt.part.${p.part}`)}</span></span><b className="tabular-nums">{p.correct}/{p.total}</b></div>
              <div className="h-1.5 rounded-full bg-gray-100 overflow-hidden">
                <div className={`h-full rounded-full anim-grow ${p.correct === p.total ? 'bg-green-500' : p.correct === 0 ? 'bg-red-400' : 'bg-blue-500'}`} style={{ width: `${(p.correct / p.total) * 100}%` }} />
              </div>
            </div>
          ))}
        </div>

        {r.missed_words.length > 0 && (
          <div className="bg-white rounded-xl shadow-sm p-4 flex items-center gap-3 anim-stag" style={{ ['--i' as string]: 3 }}>
            <div className="flex-1 min-w-0">
              <b className="text-sm">{t('jlpt.missedWords', { count: r.missed_words.length })}</b>
              <div className="text-xs text-gray-500 truncate">{r.missed_words.join('、')}</div>
            </div>
            <button className={cardsAdded ? secondaryBtn : primaryBtn} disabled={cardsAdded} onClick={addMissedWords}>
              {cardsAdded ? <><CheckOutlined className="mr-1 anim-pop" />{t('jlpt.added')}</> : t('jlpt.addToCards')}
            </button>
          </div>
        )}

        <h3 className="text-sm font-medium text-gray-500 uppercase tracking-wide px-1">{t('jlpt.answers')}</h3>
        {r.parts.map((p, pi) => (
          <div key={pi} className="flex flex-col gap-3">
            {p.passage && (
              <details className="bg-white border border-gray-200 rounded-md p-3 text-sm">
                <summary className="cursor-pointer text-gray-500">{`問題${pi + 1}`} · {t('jlpt.showPassage')}</summary>
                <div className="font-serif leading-loose whitespace-pre-wrap mt-2"><Marked text={p.passage} /></div>
              </details>
            )}
            {p.questions.map((q, qi) => {
              n += 1
              const ok = q.chosen === q.answer
              const spoken = SPOKEN_CHOICE_PARTS.includes(p.part)
              const label = (k: number | null) => (k === null ? t('jlpt.blank') : `${k + 1}. ${q.options[k]}`)
              return (
                <div key={qi} className={`border-l-[3px] pl-3 py-1 flex flex-col gap-1.5 ${ok ? 'border-green-500' : 'border-red-400'}`}>
                  <div className="flex justify-between items-center">
                    <span className="text-xs font-bold text-gray-400">{n} · {`問題${pi + 1}`}</span>
                    <span className={`text-[11px] px-2 py-0.5 rounded-full font-medium ${ok ? 'bg-green-50 text-green-600' : q.chosen === null ? 'bg-gray-100 text-gray-500' : 'bg-red-50 text-red-500'}`}>
                      {ok ? t('jlpt.correct') : q.chosen === null ? t('jlpt.blank') : t('jlpt.wrong')}
                    </span>
                  </div>
                  {(q.stem || q.script.length > 0) && (
                    <div className="font-serif text-[15px] leading-loose">
                      {q.stem ? <Marked text={q.stem} /> : null}
                      {(spoken || !q.stem) && q.script.length > 0 && <div className="text-sm text-gray-500">{q.script.join(' ')}</div>}
                    </div>
                  )}
                  <div className="text-sm flex flex-wrap gap-x-3">
                    <span>{t('jlpt.answer')} <b><Marked text={label(q.answer)} /></b></span>
                    {!ok && <span className="text-gray-500">{t('jlpt.yours')} <Marked text={label(q.chosen)} /></span>}
                  </div>
                  {q.explanation && <p className="text-sm text-gray-500">{q.explanation}</p>}
                </div>
              )
            })}
          </div>
        ))}

        {nextPlan && (
          <div className="bg-white rounded-xl shadow-sm p-4 flex flex-col gap-3">
            <span className="text-xs text-gray-400">{t('jlpt.nextInRotation')}</span>
            <b>{SECTION_JA[nextPlan.section]} · {t(`jlpt.section.${nextPlan.section}`)} · {t('jlpt.questions', { count: nextPlan.questions })} · {clock(nextPlan.time_limit)}</b>
            <div className="grid grid-cols-2 gap-2">
              <button className={secondaryBtn} onClick={() => navigate('/')}>{t('jlpt.doneForToday')}</button>
              <button className={primaryBtn} onClick={() => openBrief(nextPlan.section)}>{t('jlpt.startNext')}</button>
            </div>
            <button className="text-sm text-gray-500" onClick={() => go('hub', 'back')}>{t('jlpt.backToPractice')}</button>
          </div>
        )}
      </div>
    )
  }

  return (
    <div className="h-full overflow-y-auto overflow-x-hidden">
      {phase === 'hub' && renderHub()}
      {phase === 'brief' && renderBrief()}
      {phase === 'exam' && renderExam()}
      {phase === 'result' && renderResult()}
      {call && <Call {...call} />}
    </div>
  )
}
