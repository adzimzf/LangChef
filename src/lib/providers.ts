/**
 * AI provider presets. Data only: adding a provider here needs no other code change.
 * OpenRouter is the default (see PRDs/Indonesian Speakers Learning Japanese.md, FR9.2).
 */
export type ProviderPreset = {
  id: string
  name: string
  apiHost: string
  /** Filled in when the user picks the preset; the model list loads after "Test". */
  defaultModel: string
  keysUrl: string
  keyPrefix: string
  /** Translation keys for the setup steps, in order. */
  steps: string[]
}

export const PROVIDER_PRESETS: ProviderPreset[] = [
  {
    id: 'openrouter',
    name: 'OpenRouter',
    apiHost: 'https://openrouter.ai/api/v1',
    defaultModel: 'openrouter/auto',
    keysUrl: 'https://openrouter.ai/keys',
    keyPrefix: 'sk-or-',
    steps: ['provider.openrouter.step1', 'provider.openrouter.step2', 'provider.openrouter.step3'],
  },
]

export const DEFAULT_PROVIDER = PROVIDER_PRESETS[0]

/** Where "Report a wrong translation" goes. */
export const TRANSLATION_REPORT_URL = 'https://github.com/adzimzf/LangChef/issues/new?labels=translation&title=Terjemahan%20salah'
