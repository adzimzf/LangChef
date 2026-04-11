import React from 'react'

/**
 * Furigana annotation format:
 * The LLM annotates kanji with readings using {kanji|reading} syntax.
 * Example: "{日本|にほん}の{食|た}べ物はおいしいです"
 *
 * This module parses that format and renders <ruby> tags.
 * When showFurigana is true, readings appear above kanji.
 * When false, <rt> is hidden via CSS and only the plain text shows.
 */

// Regex to match {kanji|reading} annotations
// Captures: group 1 = the kanji text, group 2 = the reading
const FURIGANA_RE = /\{([^|}]+)\|([^}]+)\}/g

/**
 * Strip all furigana annotations from text, returning plain Japanese text.
 * Example: "{日本|にほん}の{食|た}べ物" → "日本の食べ物"
 */
export function stripFurigana(text: string): string {
  return text.replace(FURIGANA_RE, '$1')
}

/**
 * Check if text contains any furigana annotations.
 */
export function hasFuriganaAnnotations(text: string): boolean {
  FURIGANA_RE.lastIndex = 0
  return FURIGANA_RE.test(text)
}

/**
 * Parse a string with {kanji|reading} annotations into structured segments.
 * Each segment is either plain text or an annotated kanji with its reading.
 */
export function parseFurigana(text: string): Array<{ text: string; reading: string | null }> {
  const result: Array<{ text: string; reading: string | null }> = []
  FURIGANA_RE.lastIndex = 0

  let lastIndex = 0
  let match: RegExpExecArray | null

  while ((match = FURIGANA_RE.exec(text)) !== null) {
    // Push any plain text before this annotation
    if (match.index > lastIndex) {
      result.push({ text: text.slice(lastIndex, match.index), reading: null })
    }

    // Push the annotated kanji + reading
    result.push({ text: match[1], reading: match[2] })

    lastIndex = FURIGANA_RE.lastIndex
  }

  // Push any remaining plain text
  if (lastIndex < text.length) {
    result.push({ text: text.slice(lastIndex), reading: null })
  }

  return result
}

/**
 * Render text with furigana annotations as React elements using <ruby> tags.
 * When showFurigana is false, the <rt> content is hidden via CSS class.
 * When showFurigana is true, readings appear above kanji.
 */
export function renderFurigana(
  text: string,
  showFurigana: boolean,
): React.ReactNode[] {
  const segments = parseFurigana(text)

  return segments.map((seg, idx) => {
    if (!seg.reading) {
      return <span key={idx}>{seg.text}</span>
    }

    return (
      <ruby key={idx} className={showFurigana ? 'furigana-show' : 'furigana-hide'}>
        {seg.text}
        <rp>(</rp><rt>{seg.reading}</rt><rp>)</rp>
      </ruby>
    )
  })
}
