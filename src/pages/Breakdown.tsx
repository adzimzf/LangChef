import { useEffect, useRef, useState, type ReactNode } from 'react'
import { invoke } from '@tauri-apps/api/core'
import {
  ArrowLeftOutlined, LoadingOutlined, DeleteOutlined, SnippetsOutlined, CloseOutlined,
  CheckCircleOutlined, CloseCircleOutlined, PartitionOutlined, ReloadOutlined,
} from '@ant-design/icons'
import type {
  BreakdownCheckResult, BreakdownParts, BreakdownRole, BreakdownSection, BreakdownSummary, SentenceBreakdown,
} from '../lib/types'
import { t, errorText, formatDay } from '../lib/i18n'
import { TARGET_LANGUAGES } from '../lib/languages'

type Phase = 'input' | 'pick' | 'result'

const LANG_STORAGE_KEY = 'breakdown_language'

/** Colors per chunk role: the same role reads the same in the sentence, the chunk list and the order rows. */
const ROLE_STYLE: Record<BreakdownRole, { chip: string; mark: string; text: string }> = {
  topic: { chip: 'bg-green-100 text-green-800', mark: 'bg-green-100 border-b-[3px] border-green-600', text: 'text-green-800' },
  subject: { chip: 'bg-orange-100 text-orange-800', mark: 'bg-orange-100 border-b-[3px] border-orange-600', text: 'text-orange-800' },
  object: { chip: 'bg-violet-100 text-violet-800', mark: 'bg-violet-100 border-b-[3px] border-violet-600', text: 'text-violet-800' },
  modifier: { chip: 'bg-amber-100 text-amber-900', mark: 'bg-amber-100 border-b-[3px] border-amber-600', text: 'text-amber-900' },
  purpose: { chip: 'bg-blue-100 text-blue-800', mark: 'bg-blue-100 border-b-[3px] border-blue-600', text: 'text-blue-800' },
  reason: { chip: 'bg-blue-100 text-blue-800', mark: 'bg-blue-100 border-b-[3px] border-blue-600', text: 'text-blue-800' },
  condition: { chip: 'bg-blue-100 text-blue-800', mark: 'bg-blue-100 border-b-[3px] border-blue-600', text: 'text-blue-800' },
  time: { chip: 'bg-sky-100 text-sky-900', mark: 'bg-sky-100 border-b-[3px] border-sky-600', text: 'text-sky-900' },
  place: { chip: 'bg-sky-100 text-sky-900', mark: 'bg-sky-100 border-b-[3px] border-sky-600', text: 'text-sky-900' },
  connector: { chip: 'bg-gray-100 text-gray-700', mark: '', text: 'text-gray-700' },
  predicate: { chip: 'bg-gray-900 text-white', mark: 'font-bold border-b-[3px] border-gray-900', text: 'text-gray-900 font-bold' },
  other: { chip: 'bg-gray-100 text-gray-700', mark: '', text: 'text-gray-800' },
}

const LAYER_BORDERS = ['border-gray-900', 'border-blue-500', 'border-orange-500', 'border-amber-600']

/** Parses SQLite's UTC `datetime('now')` text. */
function sqliteDate(value: string): Date {
  return new Date(value.includes('T') ? value : `${value.replace(' ', 'T')}Z`)
}

/** Grey bars standing in for a section while its request runs. */
function Skeleton({ lines = 2 }: { lines?: number }) {
  return (
    <div className="flex flex-col gap-2 animate-pulse" aria-hidden="true">
      {Array.from({ length: lines }, (_, i) => (
        <div key={i} className={`h-4 rounded bg-gray-200 ${i === lines - 1 ? 'w-2/3' : 'w-full'}`} />
      ))}
    </div>
  )
}

export default function Breakdown() {
  const [phase, setPhase] = useState<Phase>('input')
  const [language, setLanguage] = useState(() => {
    const stored = localStorage.getItem(LANG_STORAGE_KEY)
    return TARGET_LANGUAGES.some((l) => l.code === stored) ? stored! : (TARGET_LANGUAGES[0]?.code ?? 'ja')
  })
  const [text, setText] = useState('')
  const [sentences, setSentences] = useState<string[]>([])
  const [history, setHistory] = useState<BreakdownSummary[]>([])
  const [result, setResult] = useState<SentenceBreakdown | null>(null)
  const [parts, setParts] = useState<Partial<BreakdownParts>>({})
  const [pending, setPending] = useState<BreakdownSection[]>([])
  const [failed, setFailed] = useState<Partial<Record<BreakdownSection, string>>>({})
  const [error, setError] = useState('')
  const [showTranslation, setShowTranslation] = useState(false)
  const [checkResult, setCheckResult] = useState<BreakdownCheckResult | null>(null)
  const [chosen, setChosen] = useState<number | null>(null)
  /** The breakdown on screen; answers for one the learner already left are dropped. */
  const shownId = useRef<number | null>(null)
  /** `id:section` requests still running, so reopening a breakdown doesn't ask the AI twice. */
  const inFlight = useRef(new Set<string>())

  const apiHost = localStorage.getItem('api_host') || ''
  const apiKey = localStorage.getItem('api_key') || ''
  const model = localStorage.getItem('api_model') || ''

  async function loadHistory(lang = language) {
    try {
      setHistory(await invoke<BreakdownSummary[]>('db_list_sentence_breakdowns', { language: lang }))
    } catch {
      setHistory([])
    }
  }

  useEffect(() => { loadHistory() }, [language])

  function pickLanguage(code: string) {
    setLanguage(code)
    localStorage.setItem(LANG_STORAGE_KEY, code)
  }

  /** Requests one section; every missing section is requested at the same time. */
  async function fetchSection(id: number, section: BreakdownSection) {
    setPending((p) => [...p, section])
    setFailed((f) => ({ ...f, [section]: undefined }))
    const key = `${id}:${section}`
    if (inFlight.current.has(key)) return
    inFlight.current.add(key)
    try {
      const value = await invoke<BreakdownParts[typeof section]>('llm_breakdown_section', { apiHost, apiKey, model, id, section })
      if (shownId.current !== id) return
      setParts((p) => ({ ...p, [section]: value }))
      if (section === 'core') loadHistory()
    } catch (e) {
      if (shownId.current === id) setFailed((f) => ({ ...f, [section]: errorText(e) }))
    } finally {
      inFlight.current.delete(key)
      if (shownId.current === id) setPending((p) => p.filter((s) => s !== section))
    }
  }

  function show(b: SentenceBreakdown) {
    shownId.current = b.id
    setResult(b)
    setParts(b.parts)
    setPending([])
    setFailed({})
    setShowTranslation(false)
    setCheckResult(null)
    setChosen(null)
    setPhase('result')
    if (b.missing.length > 0 && !apiKey) { setError(t('common.apiMissing')); return }
    setError('')
    b.missing.forEach((section) => fetchSection(b.id, section))
  }

  async function analyze(sentence: string) {
    if (!apiKey) { setError(t('common.apiMissing')); return }
    setError('')
    try {
      show(await invoke<SentenceBreakdown>('db_start_sentence_breakdown', { sentence, language }))
      loadHistory()
    } catch (e) {
      setError(errorText(e))
    }
  }

  async function submit() {
    const found = await invoke<string[]>('breakdown_split_sentences', { text })
    if (found.length === 0) return
    setSentences(found)
    if (found.length === 1) analyze(found[0])
    else { setError(''); setPhase('pick') }
  }

  async function paste() {
    try {
      const clip = await navigator.clipboard.readText()
      if (clip) setText(clip)
    } catch { /* clipboard not available: the user can still paste by hand */ }
  }

  async function openSaved(id: number) {
    const b = await invoke<SentenceBreakdown | null>('db_get_sentence_breakdown', { id })
    if (b) show(b)
  }

  async function remove(id: number) {
    await invoke('db_delete_sentence_breakdown', { id })
    loadHistory()
  }

  async function answer(choice: number) {
    if (!result || checkResult) return
    setChosen(choice)
    try {
      const r = await invoke<BreakdownCheckResult>('db_check_breakdown_answer', { id: result.id, choice })
      setCheckResult(r)
      loadHistory()
    } catch (e) {
      setError(errorText(e))
    }
  }

  function backToInput() {
    shownId.current = null
    setError('')
    setPhase('input')
  }

  /** A section's frame: its content once it arrived, a skeleton while it loads, or the error with a retry. */
  function section(name: BreakdownSection, title: string | null, body: ReactNode, lines = 2) {
    const loaded = parts[name] !== undefined
    const failure = failed[name]
    if (!loaded && !failure && !pending.includes(name)) return null
    return (
      <section className="flex flex-col gap-2">
        {title && <h2 className="text-[15px] font-bold text-gray-900">{title}</h2>}
        {loaded ? body : failure ? (
          <div className="flex items-center justify-between gap-3 bg-red-50 border border-red-200 rounded-xl px-3.5 py-2.5 text-sm text-red-700">
            <span>{failure}</span>
            <button
              onClick={() => result && fetchSection(result.id, name)}
              className="shrink-0 flex items-center gap-1.5 font-semibold text-red-700 active:text-red-900"
            >
              <ReloadOutlined /> {t('common.retry')}
            </button>
          </div>
        ) : <Skeleton lines={lines} />}
      </section>
    )
  }

  // ─── Pick a sentence ───────────────────────────────────────────────────────
  if (phase === 'pick') {
    return (
      <div className="h-full overflow-y-auto">
        <div className="flex flex-col pt-6 pb-6 gap-4 max-w-lg mx-auto">
          <div className="flex items-center gap-3">
            <button onClick={backToInput} aria-label={t('common.back')} className="text-gray-400 active:text-gray-600 p-1">
              <ArrowLeftOutlined className="text-xl" />
            </button>
            <h1 className="text-xl font-bold text-gray-900">{t('breakdown.pickTitle')}</h1>
          </div>
          <p className="text-sm text-gray-600">{t('breakdown.pickHint', { count: sentences.length })}</p>
          {error && <div className="bg-red-50 border border-red-200 rounded-xl px-4 py-3 text-sm text-red-700">{error}</div>}
          <div className="flex flex-col gap-2">
            {sentences.map((s, i) => (
              <button
                key={i}
                onClick={() => analyze(s)}
                className="flex gap-3 items-start text-left bg-white rounded-2xl border border-gray-100 shadow-sm px-4 py-3 active:bg-gray-50"
              >
                <span className="shrink-0 w-6 h-6 rounded-full bg-gray-100 text-gray-600 text-xs font-bold flex items-center justify-center mt-0.5">
                  {i + 1}
                </span>
                <span className="text-[16px] leading-relaxed text-gray-900">{s}</span>
              </button>
            ))}
          </div>
        </div>
      </div>
    )
  }

  // ─── Result ────────────────────────────────────────────────────────────────
  if (phase === 'result' && result) {
    const { chunks, core, layers, order, notes, gloss, check } = parts
    const said = chunks?.chunks.filter((c) => !c.implied) ?? []
    const rolesShown = [...new Set(said.map((c) => c.role))].filter((role) => role !== 'other' && role !== 'connector')
    return (
      <div className="h-full overflow-y-auto">
        <div className="flex flex-col pt-6 pb-8 gap-6 max-w-lg mx-auto">
          <div className="flex items-center gap-3">
            <button onClick={backToInput} aria-label={t('common.back')} className="text-gray-400 active:text-gray-600 p-1">
              <ArrowLeftOutlined className="text-xl" />
            </button>
            <h1 className="flex-1 text-xl font-bold text-gray-900">{t('breakdown.resultTitle')}</h1>
            {pending.length > 0 && <LoadingOutlined className="text-gray-400" spin aria-label={t('breakdown.loading')} />}
          </div>

          {/* The sentence: shown at once, colored by chunk when the chunks arrive */}
          <section className="flex flex-col gap-2">
            <p className="text-[19px] leading-[2.3] text-gray-900">
              {chunks?.chunks_match
                ? chunks.chunks.map((c, i) => c.implied
                  ? <span key={i} className="mx-0.5 px-1 rounded border border-dashed border-gray-400 text-gray-500 text-sm">{c.text}</span>
                  : <span key={i} className={`px-0.5 rounded ${ROLE_STYLE[c.role].mark}`}>{c.text}</span>)
                : result.sentence}
            </p>
            {chunks?.chunks_match && (
              <div className="flex flex-wrap gap-x-3 gap-y-1 text-xs text-gray-600">
                {rolesShown.map((role) => (
                  <span key={role} className="flex items-center gap-1.5">
                    <span className={`w-3 h-3 rounded-sm ${ROLE_STYLE[role].chip}`} />
                    {t(`breakdown.role.${role}`)}
                  </span>
                ))}
                {chunks.chunks.some((c) => c.implied) && (
                  <span className="flex items-center gap-1.5">
                    <span className="w-3 h-3 rounded-sm border border-dashed border-gray-500" />
                    {t('breakdown.implied')}
                  </span>
                )}
              </div>
            )}
          </section>

          {error && <div className="bg-red-50 border border-red-200 rounded-xl px-4 py-3 text-sm text-red-700">{error}</div>}

          {section('core', null, core && (
            <div className="bg-blue-50 rounded-2xl px-4 py-3.5 flex flex-col gap-1.5">
              <p className="text-[11px] font-bold text-blue-700 uppercase tracking-wide">{t('breakdown.core')}</p>
              <p className="text-[17px] font-bold text-gray-900 leading-snug">{core.core}</p>
              {core.core_note && <p className="text-sm text-gray-700 leading-relaxed">{core.core_note}</p>}
              {core.tip && <p className="text-[13px] text-gray-600 leading-relaxed">{core.tip}</p>}
            </div>
          ), 3)}

          {section('layers', t('breakdown.layers'), layers && layers.layers.reduceRight<ReactNode>((inner, layer, i) => (
            <div className={`border-2 ${LAYER_BORDERS[i % LAYER_BORDERS.length]} rounded-xl p-3 flex flex-col gap-1.5 bg-white`}>
              <span className="text-[15px] text-gray-900">{layer.text}</span>
              <span className="text-xs text-gray-600">{layer.meaning}</span>
              {inner}
            </div>
          ), null), 4)}

          {section('order', t('breakdown.order'), order && (
            <>
              <p className="text-xs text-gray-500">{t('breakdown.orderTarget')}</p>
              <div className="flex flex-wrap gap-1.5">
                {order.target.map((p, i) => (
                  <span key={i} className={`text-[13px] px-2.5 py-1.5 rounded-lg ${ROLE_STYLE[p.role].chip}`}>{p.text}</span>
                ))}
              </div>
              <p className="text-xs text-gray-500 mt-1">{t('breakdown.orderNative')}</p>
              <div className="flex flex-wrap gap-1.5">
                {order.native.map((p, i) => (
                  <span key={i} className={`text-[13px] px-2.5 py-1.5 rounded-lg ${ROLE_STYLE[p.role].chip}`}>{p.text}</span>
                ))}
              </div>
              {order.note && <p className="text-[13px] text-gray-600 leading-relaxed mt-1">{order.note}</p>}
            </>
          ), 3)}

          {section('chunks', t('breakdown.chunks'), chunks && chunks.chunks.map((c, i) => (
            <div key={i} className="bg-white border border-gray-100 shadow-sm rounded-2xl px-4 py-3 flex flex-col gap-1">
              <div className="flex justify-between items-start gap-2">
                <span className={`text-[16px] ${c.implied ? 'text-gray-500' : ROLE_STYLE[c.role].text}`}>{c.text}</span>
                {c.label && (
                  <span className={`shrink-0 text-[11px] font-semibold px-2 py-0.5 rounded-full ${ROLE_STYLE[c.role].chip}`}>{c.label}</span>
                )}
              </div>
              {c.meaning && <span className="text-sm text-gray-600">{c.meaning}</span>}
            </div>
          )), 4)}

          {section('notes', t('breakdown.notes'), notes && (
            <div className="grid grid-cols-[minmax(0,7rem)_minmax(0,1fr)] gap-x-3 gap-y-2.5 text-sm leading-relaxed">
              {notes.notes.map((n, i) => (
                <div key={i} className="contents">
                  <span className="text-[15px] font-semibold text-gray-900 break-words">{n.text}</span>
                  <span className="text-gray-700">{n.note}</span>
                </div>
              ))}
            </div>
          ), 4)}

          {section('gloss', t('breakdown.gloss'), gloss && (
            <div className="flex flex-wrap gap-1.5">
              {gloss.gloss.map((g, i) => (
                <div key={i} className="flex flex-col items-center bg-white border border-gray-100 rounded-lg px-2 py-1.5">
                  {g.reading && g.reading !== g.text && <span className="text-[10px] text-gray-500">{g.reading}</span>}
                  <span className="text-[15px] text-gray-900">{g.text}</span>
                  <span className="text-[11px] text-gray-600">{g.meaning}</span>
                </div>
              ))}
            </div>
          ), 2)}

          {section('check', null, check && (
            <div className="bg-white border border-gray-100 shadow-sm rounded-2xl px-4 py-4 flex flex-col gap-3">
              <p className="text-[11px] font-bold text-blue-700 uppercase tracking-wide">{t('breakdown.check')}</p>
              <p className="text-[17px] font-bold text-gray-900 leading-snug">{check.question}</p>
              <div className="flex flex-col gap-2">
                {check.options.map((o, i) => {
                  const isAnswer = checkResult?.answer === i
                  const isWrongPick = checkResult && chosen === i && !checkResult.correct
                  return (
                    <button
                      key={i}
                      onClick={() => answer(i)}
                      disabled={!!checkResult}
                      className={`flex items-center justify-between gap-2 text-left px-4 py-3 rounded-xl border text-[15px]
                        ${isAnswer ? 'bg-green-50 border-green-600 text-green-800 font-semibold'
                          : isWrongPick ? 'bg-red-50 border-red-600 text-red-800 font-semibold'
                            : 'bg-white border-gray-200 text-gray-800 active:bg-gray-50'}`}
                    >
                      <span>{o}</span>
                      {isAnswer && <CheckCircleOutlined />}
                      {isWrongPick && <CloseCircleOutlined />}
                    </button>
                  )
                })}
              </div>
              {checkResult && (
                <div className={`rounded-xl px-3.5 py-3 text-sm leading-relaxed ${checkResult.correct ? 'bg-green-50 text-green-900' : 'bg-orange-50 text-orange-900'}`}>
                  <p className="font-bold mb-0.5">{checkResult.correct ? t('breakdown.checkRight') : t('breakdown.checkWrong')}</p>
                  {checkResult.explanation}
                </div>
              )}
              {!checkResult && result.checked !== null && (
                <p className="text-xs text-gray-500">{result.checked ? t('breakdown.checkedRight') : t('breakdown.checkedWrong')}</p>
              )}
            </div>
          ), 3)}

          <div className="flex flex-col gap-2">
            {core?.translation && (
              <button
                onClick={() => setShowTranslation((v) => !v)}
                className="w-full py-3 rounded-xl border border-gray-200 bg-white text-gray-700 font-semibold text-[15px] active:bg-gray-50"
              >
                {showTranslation ? t('breakdown.hideTranslation') : t('breakdown.showTranslation')}
              </button>
            )}
            {showTranslation && core && (
              <p className="text-[15px] text-gray-800 bg-white rounded-xl border border-gray-100 px-4 py-3 leading-relaxed">{core.translation}</p>
            )}
            <button
              onClick={() => { setText(''); backToInput() }}
              className="w-full py-3 rounded-xl text-blue-600 font-semibold text-[15px] active:text-blue-700"
            >
              {t('breakdown.another')}
            </button>
          </div>
        </div>
      </div>
    )
  }

  // ─── Input + history ───────────────────────────────────────────────────────
  return (
    <div className="h-full overflow-y-auto">
      <div className="flex flex-col pt-6 pb-6 gap-5 max-w-lg mx-auto">
        <div>
          <h1 className="text-2xl font-bold text-gray-900">{t('breakdown.title')}</h1>
          <p className="text-sm text-gray-500 mt-0.5">{t('breakdown.subtitle')}</p>
        </div>

        {TARGET_LANGUAGES.length > 1 && (
          <div className="flex gap-2">
            {TARGET_LANGUAGES.map((l) => (
              <button
                key={l.code}
                onClick={() => pickLanguage(l.code)}
                className={`flex-1 py-2.5 rounded-xl border text-[15px] font-medium flex items-center justify-center gap-2
                  ${language === l.code ? 'bg-blue-50 border-blue-300 text-blue-700' : 'bg-white border-gray-200 text-gray-700 active:bg-gray-50'}`}
              >
                <span>{l.flag}</span>{l.label}
              </button>
            ))}
          </div>
        )}

        <div className="bg-white rounded-2xl border-2 border-blue-500 px-4 py-3 flex flex-col gap-2">
          <label htmlFor="breakdown-text" className="text-[11px] font-bold text-blue-700 uppercase tracking-wide">
            {t('breakdown.inputLabel')}
          </label>
          <textarea
            id="breakdown-text"
            value={text}
            onChange={(e) => setText(e.target.value)}
            rows={5}
            placeholder={t('breakdown.placeholder')}
            className="w-full resize-none outline-none text-[17px] leading-relaxed text-gray-900 placeholder:text-gray-400 bg-transparent"
          />
          <div className="flex justify-end gap-1">
            <button
              onClick={paste}
              className="flex items-center gap-1.5 px-3 py-2 rounded-lg bg-blue-50 text-blue-700 text-[13px] font-semibold active:bg-blue-100"
            >
              <SnippetsOutlined /> {t('breakdown.paste')}
            </button>
            {text && (
              <button
                onClick={() => setText('')}
                aria-label={t('breakdown.clear')}
                className="w-9 h-9 rounded-lg text-gray-500 active:bg-gray-100 flex items-center justify-center"
              >
                <CloseOutlined />
              </button>
            )}
          </div>
        </div>

        {error && <div className="bg-red-50 border border-red-200 rounded-xl px-4 py-3 text-sm text-red-700">{error}</div>}

        <button
          onClick={submit}
          disabled={!text.trim()}
          className="w-full py-3.5 rounded-xl bg-blue-500 text-white font-semibold text-[15px] active:bg-blue-600
            disabled:bg-gray-200 disabled:text-gray-400 flex items-center justify-center gap-2"
        >
          <PartitionOutlined /> {t('breakdown.submit')}
        </button>

        <section className="flex flex-col gap-2 anim-stagger">
          <h2 className="text-[15px] font-bold text-gray-900">{t('breakdown.recent')}</h2>
          {history.length === 0 ? (
            <p className="text-sm text-gray-500">{t('breakdown.recentEmpty')}</p>
          ) : (
            history.map((h) => (
              <div key={h.id} className="flex items-stretch bg-white rounded-2xl border border-gray-100 shadow-sm">
                <button onClick={() => openSaved(h.id)} className="flex-1 min-w-0 text-left px-4 py-3 flex flex-col gap-1.5 active:bg-gray-50 rounded-l-2xl">
                  <span className="text-[15px] text-gray-900 truncate">{h.sentence}</span>
                  <span className="flex flex-wrap items-center gap-1.5 text-[11px] text-gray-500">
                    {h.structures.map((s) => (
                      <span key={s} className="font-semibold bg-orange-100 text-orange-800 px-2 py-0.5 rounded-full">{s}</span>
                    ))}
                    {h.checked !== null && (
                      <span className={h.checked ? 'text-green-700' : 'text-orange-700'}>
                        {h.checked ? <CheckCircleOutlined /> : <CloseCircleOutlined />}
                      </span>
                    )}
                    <span>{formatDay(sqliteDate(h.created_at))}</span>
                  </span>
                </button>
                <button
                  onClick={() => remove(h.id)}
                  aria-label={t('common.delete')}
                  className="px-3 text-gray-400 active:text-red-500"
                >
                  <DeleteOutlined />
                </button>
              </div>
            ))
          )}
        </section>
      </div>
    </div>
  )
}
