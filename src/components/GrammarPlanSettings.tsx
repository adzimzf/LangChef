import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { MinusOutlined, PlusOutlined } from '@ant-design/icons'
import type { GrammarPlan } from '../lib/types'
import { t, errorText } from '../lib/i18n'
import { TARGET_LANGUAGES } from '../lib/languages'

type PlanChange = { level?: string; newPerDay?: number; checksPerDay?: number; speedUp?: boolean }

function Stepper({ label, hint, value, onChange }: { label: string; hint: string; value: number; onChange: (n: number) => void }) {
  const btn = 'w-10 h-10 shrink-0 flex items-center justify-center rounded-xl border-[1.5px] border-gray-200 text-gray-700 hover:bg-gray-50 disabled:opacity-40'
  return (
    <div className="px-4 py-3 border-b border-gray-50 flex items-center gap-3">
      <div className="flex-1 min-w-0">
        <div className="text-sm text-gray-700">{label}</div>
        <div className="text-xs text-gray-400">{hint}</div>
      </div>
      <button type="button" className={btn} disabled={value <= 0} onClick={() => onChange(value - 1)} aria-label={t('grammarPlan.fewer', { what: label })}>
        <MinusOutlined />
      </button>
      <span className="w-6 text-center text-[15px] font-semibold tabular-nums">{value}</span>
      <button type="button" className={btn} onClick={() => onChange(value + 1)} aria-label={t('grammarPlan.more', { what: label })}>
        <PlusOutlined />
      </button>
    </div>
  )
}

/** Grammar review: the learner's level per target language and the daily grammar limits. Saved on every change. */
export default function GrammarPlanSettings() {
  const [language, setLanguage] = useState(TARGET_LANGUAGES[0]?.code ?? 'ja')
  const [plan, setPlan] = useState<GrammarPlan | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    invoke<GrammarPlan>('db_get_grammar_plan', { language })
      .then(setPlan)
      .catch(e => setError(errorText(e)))
  }, [language])

  async function update(change: PlanChange) {
    try {
      setPlan(await invoke<GrammarPlan>('db_set_grammar_plan', { language, ...change }))
      setError(null)
    } catch (e) {
      setError(errorText(e))
    }
  }

  if (!plan || plan.levels.length === 0) return null
  const level = plan.level ?? ''
  const current = plan.levels.find(l => l.role === 'current')
  const below = plan.levels.filter(l => l.role === 'below').map(l => l.level)
  const above = plan.levels.filter(l => l.role === 'above').map(l => l.level)

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between px-1">
        <h3 className="text-sm font-medium text-gray-500 uppercase tracking-wide">{t('grammarPlan.settingsTitle')}</h3>
        {TARGET_LANGUAGES.length > 1 && (
          <div className="flex gap-1 bg-white rounded-lg p-0.5 shadow-sm">
            {TARGET_LANGUAGES.map(l => (
              <button
                key={l.code}
                type="button"
                onClick={() => setLanguage(l.code)}
                className={`px-2 py-1 rounded-md text-xs font-medium ${language === l.code ? 'bg-blue-500 text-white' : 'text-gray-600'}`}
              >
                {l.flag} {l.label}
              </button>
            ))}
          </div>
        )}
      </div>

      <div className="bg-white rounded-xl shadow-sm overflow-hidden">
        <div className="px-4 py-3 border-b border-gray-50 space-y-3">
          <div className="text-sm text-gray-500">{t('grammarPlan.yourLevel')}</div>
          <div className="flex gap-1.5 flex-wrap">
            {plan.levels.map(l => (
              <button
                key={l.level}
                type="button"
                aria-pressed={l.level === level}
                onClick={() => update({ level: l.level })}
                className={`min-w-[3.25rem] h-10 px-3 rounded-xl text-sm font-semibold ${
                  l.level === level ? 'bg-blue-500 text-white' : 'bg-white text-gray-800 border-[1.5px] border-gray-200 hover:bg-gray-50'
                }`}
              >
                {l.level}
              </button>
            ))}
          </div>
          <ul className="space-y-1.5 text-xs text-gray-600">
            <li className="flex gap-2">
              <span className="mt-1 w-2 h-2 shrink-0 rounded-full bg-blue-500" />
              <span>{t('grammarPlan.explainCurrent', { level, count: current?.total ?? 0 })}</span>
            </li>
            <li className="flex gap-2">
              <span className="mt-1 w-2 h-2 shrink-0 rounded-full bg-amber-600" />
              <span>{below.length > 0 ? t('grammarPlan.explainBelow', { levels: below.join(', ') }) : t('grammarPlan.explainNoBelow', { level })}</span>
            </li>
            {above.length > 0 && (
              <li className="flex gap-2">
                <span className="mt-1 w-2 h-2 shrink-0 rounded-full bg-gray-400" />
                <span>{t('grammarPlan.explainAbove', { levels: above.join(', ') })}</span>
              </li>
            )}
          </ul>
          <p className="text-xs text-gray-400">{t('grammarPlan.keepSchedule')}</p>
        </div>

        <Stepper
          label={t('grammarPlan.newPerDay')}
          hint={t('grammarPlan.newPerDayHint', { level })}
          value={plan.new_per_day}
          onChange={n => update({ newPerDay: n })}
        />
        <Stepper
          label={t('grammarPlan.checksPerDay')}
          hint={below.length > 0 ? t('grammarPlan.checksPerDayHint', { levels: below.join(', ') }) : t('grammarPlan.explainNoBelow', { level })}
          value={plan.checks_per_day}
          onChange={n => update({ checksPerDay: n })}
        />
        <div className="px-4 py-3 flex items-center gap-3">
          <div className="flex-1 min-w-0">
            <div className="text-sm text-gray-700">{t('grammarPlan.speedUp')}</div>
            <div className="text-xs text-gray-400">{t('grammarPlan.speedUpHint')}</div>
          </div>
          <button
            type="button"
            role="switch"
            aria-checked={plan.speed_up}
            aria-label={t('grammarPlan.speedUp')}
            onClick={() => update({ speedUp: !plan.speed_up })}
            className={`w-12 h-7 shrink-0 rounded-full p-0.5 flex transition-colors ${plan.speed_up ? 'bg-blue-500 justify-end' : 'bg-gray-300 justify-start'}`}
          >
            <span className="w-6 h-6 rounded-full bg-white shadow-sm" />
          </button>
        </div>
      </div>

      <div className="bg-blue-50 rounded-xl px-4 py-3 space-y-1 text-xs text-gray-700">
        <div className="font-semibold text-blue-900">{t('grammarPlan.atThisPace')}</div>
        <div>
          {plan.to_introduce === 0
            ? t('grammarPlan.allIntroduced', { level })
            : plan.days_to_introduce != null
              ? t('grammarPlan.forecastNew', { n: plan.to_introduce, level, count: plan.days_to_introduce })
              : t('grammarPlan.noNewWhileZero', { level })}
        </div>
        {below.length > 0 && (
          <div>
            {plan.to_check === 0
              ? t('grammarPlan.allChecked', { levels: below.join(', ') })
              : plan.days_to_check != null
                ? t('grammarPlan.forecastCheck', { n: plan.to_check, levels: below.join(', '), count: plan.days_to_check })
                : t('grammarPlan.noChecksWhileZero')}
          </div>
        )}
      </div>
      {error && <p className="text-xs text-red-500 px-1">{error}</p>}
    </div>
  )
}
