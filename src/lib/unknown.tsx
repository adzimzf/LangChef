const PUNCT = /[.,!?;:()"'""\[\]{}。、！？「」『』（）・]/

// CJK Unified Ideographs, Hiragana, Katakana, Hangul syllables, etc.
const CJK_RANGE = /[\u3000-\u303f\u3040-\u309f\u30a0-\u30ff\u4e00-\u9faf\uac00-\ud7af\uff00-\uffef]/

export function isCJK(char: string): boolean {
  return CJK_RANGE.test(char)
}

function hasCJK(text: string): boolean {
  return CJK_RANGE.test(text)
}

// Cache segmenters per locale for performance
const segmenterCache = new Map<string, Intl.Segmenter>()

function getSegmenter(locale: string): Intl.Segmenter {
  if (!segmenterCache.has(locale)) {
    segmenterCache.set(locale, new Intl.Segmenter(locale, { granularity: 'word' }))
  }
  return segmenterCache.get(locale)!
}

/**
 * Split text into tappable word segments.
 *
 * For CJK text (Japanese, Chinese, Korean): uses Intl.Segmenter for proper
 * morphological word boundary detection. "食べました" → ["食べ", "ました"]
 *
 * For non-CJK text: splits by whitespace as usual.
 */
export function splitTappable(text: string, locale?: string): string[] {
  if (!text) return []

  // If text contains CJK characters, use Intl.Segmenter for word boundaries
  if (hasCJK(text)) {
    const lang = locale || 'ja'
    const segmenter = getSegmenter(lang)
    const segments = segmenter.segment(text)
    const result: string[] = []
    for (const seg of segments) {
      // Keep word-like segments as tappable units
      // Skip pure whitespace/punctuation segments but still include them for rendering
      result.push(seg.segment)
    }
    return result
  }

  // Non-CJK: split by whitespace, preserving spaces for rendering
  const result: string[] = []
  let i = 0
  while (i < text.length) {
    const ch = text[i]
    if (/\s/.test(ch)) {
      let ws = ''
      while (i < text.length && /\s/.test(text[i])) {
        ws += text[i]
        i++
      }
      result.push(ws)
    } else {
      let word = ''
      while (i < text.length && !/\s/.test(text[i])) {
        word += text[i]
        i++
      }
      result.push(word)
    }
  }
  return result
}

/**
 * Check if a segment is a "word" (tappable).
 * Intl.Segmenter marks word segments with isWordLike=true.
 * For non-CJK whitespace splits, any non-whitespace segment is a word.
 */
export function isWordSegment(text: string): boolean {
  return !!text.trim() && !PUNCT.test(text)
}

export function tokenize(text: string): string[] {
  if (hasCJK(text)) {
    const segmenter = getSegmenter('ja')
    const segments = segmenter.segment(text)
    const words: string[] = []
    for (const seg of segments) {
      if (seg.isWordLike) {
        words.push(seg.segment)
      }
    }
    return words
  }
  return text.replace(PUNCT, ' ').split(/\s+/).filter(Boolean)
}

export function detectUnknowns(text: string, known: Set<string>): string[] {
  const toks = tokenize(text).map(t => t.toLowerCase())
  return toks.filter(t => !known.has(t))
}

export function highlightUnknowns(text: string, unknowns: Set<string>): JSX.Element {
  const parts = splitTappable(text)
  return (
    <>
      {parts.map((p, idx) => {
        if (p.trim() && unknowns.has(p.toLowerCase())) {
          return <mark key={idx} style={{ background: '#fde68a' }}>{p}</mark>
        }
        return <span key={idx}>{p}</span>
      })}
    </>
  )
}
