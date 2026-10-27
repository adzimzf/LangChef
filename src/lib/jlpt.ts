import { invoke } from '@tauri-apps/api/core'
import { t } from './i18n'

/**
 * JLPT level definitions for Japanese conversation adaptation.
 *
 * Each level defines constraints the LLM must follow so the learner
 * is never overwhelmed by grammar or sentence length beyond their ability.
 */

export type JLPTLevel = 'N5' | 'N4' | 'N3' | 'N2' | 'N1'

export interface JLPTLevelInfo {
  /** Display label */
  label: string
  /** Short description shown in the picker, in the learner's language */
  description: string
  /** Maximum number of clauses/bunsetsu per sentence */
  maxClauses: number
  /** Maximum approximate character count per sentence */
  maxChars: number
  /**
   * Instruction fragment injected into the system prompt.
   * Must cover grammar scope and sentence-length rule.
   */
  promptRule: string
}

export const JLPT_LEVELS: Record<JLPTLevel, JLPTLevelInfo> = {
  N5: {
    label: 'N5',
    description: t('jlpt.N5'),
    maxClauses: 1,
    maxChars: 25,
    promptRule: `JLPT N5 CONSTRAINTS:
- Use ONLY grammar patterns from JLPT N5: です/ます forms, basic particles (は、が、を、に、で、も), simple て-form, basic adjectives (い/な), existence verbs (いる/ある), simple past tense, basic question forms, たい, ましょう, basic counters, simple conjunctions (そして、でも).
- Do NOT use: causative, passive, conditional beyond たら, volitional, honorifics, relative clauses, compound sentences.
- Sentences MUST be short: 1 clause only, under 25 characters. Break ideas into separate short sentences.
- Use common everyday vocabulary. Avoid abstract or literary words.
- Example acceptable: 「これは本です。」「日本に行きます。」「おいしいですか？」
- Example UNacceptable: 「昨日買った本を読んだら面白かったです。」(relative clause + たら, too complex)`,
  },
  N4: {
    label: 'N4',
    description: t('jlpt.N4'),
    maxClauses: 2,
    maxChars: 40,
    promptRule: `JLPT N4 CONSTRAINTS:
- Use grammar up to JLPT N4: N5 + たら/れば conditionals, と思います, たい/ほしい, ている/てある, ことができる, なければならない, までに, ながら, し～し, simple relative clauses (one level), すぎる, はず, ように, basic potential form.
- Do NOT use: causative-passive, advanced conditionals (ものの、わりに), formal honorifics (謙譲語/尊敬語), complex compound sentences with 3+ clauses.
- Sentences: at most 2 clauses, under 40 characters. Prefer breaking longer ideas into 2 short sentences.
- Example acceptable: 「日本に行ったら、おいしいものを食べたいです。」「毎日勉強しなければなりません。」
- Example UNacceptable: 「日本に行ったことがある人に聞けばよかったのに、聞き忘れてしまいました。」(3+ clauses, てしまう, のに)`,
  },
  N3: {
    label: 'N3',
    description: t('jlpt.N3'),
    maxClauses: 3,
    maxChars: 60,
    promptRule: `JLPT N3 CONSTRAINTS:
- Use grammar up to JLPT N3: N4 + ばかり、ざるを得ない、わけにはいかない、に違いない、もらえる、させていただく、ことになる、ということだ、ような気する、に対して、にとって、として、に比べて、basic passive and causative forms, conditionals with なら/ば, ようにする/なる, てしまう, simple keigo (丁寧語).
- Do NOT use: advanced literary grammar (べく、まじき), complex keigo chains, multi-level embedded clauses.
- Sentences: at most 3 clauses, under 60 characters. You may use compound sentences but keep them readable.
- Example acceptable: 「日本の文化について調べたことがありますが、まだまだ知らないことが多いです。」「先生に相談させていただきたいのですが。」`,
  },
  N2: {
    label: 'N2',
    description: t('jlpt.N2'),
    maxClauses: 4,
    maxChars: 80,
    promptRule: `JLPT N2 CONSTRAINTS:
- Use grammar up to JLPT N2: N3 + べき、ものの、わりに、どころか、かねない、ざるを得ない、にすぎない、をもとに、いかにも、かというと、ようにもならない、つつ、かねる、までもない、ことなしに、full keigo (尊敬語・謙譲語・丁寧語).
- Do NOT use: literary/academic-only patterns (べく、まじき、ごとき), archaic forms.
- Sentences: at most 4 clauses, under 80 characters. You may use complex compound and embedded sentences.
- Example acceptable: 「彼の説明を聞いたものの、まだ完全には理解できていません。」「ご相談させていただいた件につきまして、こちらからご提案がございます。」`,
  },
  N1: {
    label: 'N1',
    description: t('jlpt.N1'),
    maxClauses: 0, // no limit
    maxChars: 0,   // no limit
    promptRule: `JLPT N1 CONSTRAINTS:
- No grammar restrictions. Use any grammar pattern freely, including literary, academic, and archaic forms.
- Keep sentences natural and varied. Avoid unnecessarily long sentences, but no hard limit.
- You may mix registers (casual, polite, formal) as appropriate to the conversation context.
- Aim for native-like fluency and nuance.`,
  },
}

/** All JLPT levels in order from easiest to hardest */
export const JLPT_LEVEL_ORDER: JLPTLevel[] = ['N5', 'N4', 'N3', 'N2', 'N1']

/** Default JLPT level for new chats */
export const DEFAULT_JLPT_LEVEL: JLPTLevel = 'N4'

/** localStorage key for persisting the last-selected JLPT level */
export const JLPT_LEVEL_STORAGE_KEY = 'jlpt_level'

/** Grammar review starts at the JLPT level picked before it existed; Rust ignores this once a level is set. */
export function seedGrammarLevel(): Promise<void> {
  let level: string | null = null
  try { level = localStorage.getItem(JLPT_LEVEL_STORAGE_KEY) } catch { /* storage blocked */ }
  return level ? invoke<void>('db_seed_grammar_level', { language: 'ja', level }).catch(() => {}) : Promise.resolve()
}

// ---------------------------------------------------------------------------
// JLPT practice (timed sections like the real test)
// ---------------------------------------------------------------------------

/** localStorage key for the level practised on the JLPT practice page */
export const JLPT_PRACTICE_LEVEL_KEY = 'jlpt_practice_level'

export function practiceLevel(): JLPTLevel {
  try {
    const v = localStorage.getItem(JLPT_PRACTICE_LEVEL_KEY) || localStorage.getItem(JLPT_LEVEL_STORAGE_KEY)
    if (v && (JLPT_LEVEL_ORDER as string[]).includes(v)) return v as JLPTLevel
  } catch { /* storage blocked */ }
  return 'N5'
}

/** Section names as the test booklet prints them, the same in every explanation language. */
export const SECTION_JA: Record<string, string> = {
  vocab: '文字・語彙',
  grammar: '文法',
  reading: '読解',
  listening: '聴解',
}

/** The instruction the test booklet prints above each 問題 type. */
export const PART_INSTRUCTION: Record<string, string> = {
  kanji_reading: '＿＿＿の ことばは ひらがなで どう かきますか。１・２・３・４から いちばん いい ものを ひとつ えらんで ください。',
  orthography: '＿＿＿の ことばは どう かきますか。１・２・３・４から いちばん いい ものを ひとつ えらんで ください。',
  word_formation: '（　　）に 入れるのに 最も よいものを、１・２・３・４から 一つ 選びなさい。',
  context: '（　　）に なにが はいりますか。１・２・３・４から いちばん いい ものを ひとつ えらんで ください。',
  paraphrase: '＿＿＿の ことばと だいたい おなじ いみの ものが あります。１・２・３・４から いちばん いい ものを ひとつ えらんで ください。',
  usage: 'つぎの ことばの つかいかたで いちばん いい ものを １・２・３・４から ひとつ えらんで ください。',
  sentence_grammar: '（　　）に 何を 入れますか。１・２・３・４から いちばん いい ものを 一つ えらんで ください。',
  sentence_order: '★ に 入る ものは どれですか。１・２・３・４から いちばん いい ものを 一つ えらんで ください。',
  text_grammar: 'つぎの 文章を 読んで、【　】に 入る いちばん いい ものを １・２・３・４から 一つ えらんで ください。',
  short_text: 'つぎの 文章を 読んで、しつもんに こたえて ください。',
  mid_text: 'つぎの 文章を 読んで、しつもんに こたえて ください。',
  long_text: 'つぎの 文章を 読んで、しつもんに こたえて ください。',
  integrated: 'つぎの AとBの 文章を 読んで、しつもんに こたえて ください。',
  thematic: 'つぎの 文章を 読んで、しつもんに こたえて ください。',
  info_search: 'つぎの ページを 見て、しつもんに こたえて ください。',
  task: 'はじめに しつもんを 聞いて ください。それから 話を 聞いて、１から４の 中から、いちばん いい ものを 一つ えらんで ください。',
  point: 'はじめに しつもんを 聞いて ください。それから 話を 聞いて、１から４の 中から、いちばん いい ものを 一つ えらんで ください。',
  summary: '話を 聞いて、しつもんに いちばん いい こたえを １から４の 中から 一つ えらんで ください。',
  integrated_listening: '話を 聞いて、しつもんに いちばん いい こたえを １から４の 中から 一つ えらんで ください。',
  utterance: 'しつもんを 聞いて ください。何と 言いますか。１から３の 中から、いちばん いい ものを 一つ えらんで ください。',
  quick_response: 'ぶんを 聞いて、１から３の 中から、いちばん いい へんじを 一つ えらんで ください。',
}

/** Parts whose choices are only spoken, like the real test. */
export const SPOKEN_CHOICE_PARTS = ['utterance', 'quick_response']
