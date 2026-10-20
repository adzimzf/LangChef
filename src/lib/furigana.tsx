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
 * Given annotated text (with {kanji|reading} syntax), a plain-text
 * character offset, and a length, extract the corresponding slice
 * of annotated text.
 *
 * Example: annotated = "abc{漢字|かんじ}def", offset=3, length=2
 *   → returns "{漢字|かんじ}" (plain chars at index 3,4 are 漢字)
 */
export function sliceAnnotated(annotated: string, plainOffset: number, plainLength: number): string {
  let plainIdx = 0
  let start = -1
  let end = -1

  for (let i = 0; i < annotated.length; ) {
    if (annotated[i] === '{') {
      // Find the closing }
      const closeIdx = annotated.indexOf('}', i)
      if (closeIdx === -1) break

      // Extract kanji part (before |)
      const inner = annotated.slice(i + 1, closeIdx)
      const pipeIdx = inner.indexOf('|')
      const kanji = pipeIdx >= 0 ? inner.slice(0, pipeIdx) : inner

      // Each kanji char counts as one plain char
      for (let k = 0; k < kanji.length; k++) {
        if (plainIdx === plainOffset && start === -1) {
          start = i
        }
        plainIdx++
        if (plainIdx === plainOffset + plainLength) {
          end = closeIdx + 1
          return annotated.slice(start, end)
        }
      }
      i = closeIdx + 1
    } else {
      if (plainIdx === plainOffset && start === -1) {
        start = i
      }
      plainIdx++
      if (plainIdx === plainOffset + plainLength) {
        end = i + 1
        return annotated.slice(start, end)
      }
      i++
    }
  }

  // Fallback: if we didn't find exact boundaries, return from start to end
  if (start !== -1) {
    return annotated.slice(start)
  }
  return ''
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
