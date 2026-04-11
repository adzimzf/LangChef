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
  /** Short description shown in the picker */
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
    description: 'Basic — simple greetings, self-introduction',
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
    description: 'Elementary — daily life, simple opinions',
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
    description: 'Intermediate — opinions, reasons, comparisons',
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
    description: 'Upper-intermediate — nuanced discussion, keigo',
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
    description: 'Advanced — natural, any register',
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
