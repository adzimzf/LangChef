import { useState } from 'react'
import { CopyOutlined, CheckOutlined } from '@ant-design/icons'
import { t } from '../lib/i18n'
import type { ProviderPreset } from '../lib/providers'

/** Step-by-step guide for getting an API key from a preset provider, with a copyable link. */
export default function ProviderGuide({ preset, onUse, active }: {
  preset: ProviderPreset
  onUse: (preset: ProviderPreset) => void
  active: boolean
}) {
  const [copied, setCopied] = useState(false)

  function copyLink() {
    navigator.clipboard?.writeText(preset.keysUrl).then(() => {
      setCopied(true)
      setTimeout(() => setCopied(false), 1500)
    }).catch(() => {})
  }

  return (
    <div className={`rounded-xl border-2 p-4 space-y-3 bg-white ${active ? 'border-blue-500' : 'border-gray-100'}`}>
      <div className="flex items-center justify-between gap-2">
        <div>
          <div className="font-semibold text-[15px]">{preset.name}</div>
          <div className="text-xs text-gray-500">{t('provider.recommended')}</div>
        </div>
        {active ? (
          <span className="text-[11px] font-semibold px-2 py-1 rounded-full bg-blue-50 text-blue-600">{t('provider.inUse')}</span>
        ) : (
          <button
            type="button"
            onClick={() => onUse(preset)}
            className="px-3 py-1.5 rounded-lg bg-blue-500 text-white text-sm font-medium hover:bg-blue-600"
          >
            {t('provider.use')}
          </button>
        )}
      </div>
      <ol className="list-decimal pl-5 space-y-1 text-sm text-gray-700">
        {preset.steps.map((key) => (
          <li key={key}>{t(key, { url: preset.keysUrl, prefix: preset.keyPrefix })}</li>
        ))}
      </ol>
      <div className="flex items-center gap-2">
        <a href={preset.keysUrl} target="_blank" rel="noreferrer" className="text-sm text-blue-600 break-all">{preset.keysUrl}</a>
        <button
          type="button"
          onClick={copyLink}
          aria-label={t('provider.copyLink')}
          className="shrink-0 w-8 h-8 flex items-center justify-center rounded-full hover:bg-gray-100 text-gray-500"
        >
          {copied ? <CheckOutlined /> : <CopyOutlined />}
        </button>
      </div>
    </div>
  )
}
