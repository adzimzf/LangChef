import { useEffect, useMemo, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { parseFurigana, renderFurigana, sliceAnnotated, stripFurigana } from '../../lib/furigana'
import { splitTappable, isWordSegment } from '../../lib/unknown'
import type { ReadingText as ReadingTextType, ReviewItem } from '../../lib/types'

// Maps backend bg classes to hex colors for inline text coloring
const BG_TO_HEX: Record<string, string> = {
  'bg-red-500': '#ef4444',
  'bg-orange-500': '#f97316',
  'bg-amber-500': '#f59e0b',
  'bg-yellow-500': '#eab308',
  'bg-lime-500': '#84cc16',
  'bg-green-500': '#22c55e',
  'bg-emerald-500': '#10b981',
}

export type TextSize = 'sm' | 'md' | 'lg'

const SIZE_CLASS: Record<TextSize, string> = {
  sm: 'text-[15px] leading-[1.9]',
  md: 'text-[18px] leading-[2.05]',
  lg: 'text-[21px] leading-[2.15]',
}

type Token = { surface: string; base_form: string }

interface Props {
  text: ReadingTextType
  reviewItems: ReviewItem[]
  showFurigana: boolean
  size?: TextSize
  /** Tapping a word: the word as written and its dictionary form. */
  onWordTap?: (word: string, base: string) => void
}

/** The passage, one paragraph per line, every word tappable; words in the deck are colored by difficulty. */
export default function ReadingText({ text, reviewItems, showFurigana, size = 'md', onWordTap }: Props) {
  const paragraphs = useMemo(() => text.body.split(/\n+/).map((p) => p.trim()).filter(Boolean), [text.body])
  // Lindera tokens per paragraph (Japanese); null until loaded or when the tokenizer is not ready.
  const [tokens, setTokens] = useState<Token[][] | null>(null)

  useEffect(() => {
    setTokens(null)
    if (text.language !== 'ja') return
    let cancelled = false
    Promise.all(paragraphs.map((p) => invoke<Token[]>('tokenize_japanese', { text: stripFurigana(p) })))
      .then((all) => {
        // Only usable when the tokens add up to the paragraph exactly.
        const ok = all.every((toks, i) => toks.map((t) => t.surface).join('') === stripFurigana(paragraphs[i]))
        if (!cancelled && ok) setTokens(all)
      })
      .catch(() => {})
    return () => { cancelled = true }
  }, [paragraphs, text.language])

  const reviewMap = useMemo(() => {
    const map = new Map<string, ReviewItem>()
    for (const item of reviewItems) {
      if (!item.language || item.language === text.language) map.set(item.content.toLowerCase(), item)
    }
    return map
  }, [reviewItems, text.language])

  function colorFor(...words: string[]): string | undefined {
    for (const w of words) {
      const item = reviewMap.get(w.toLowerCase())
      if (item) return BG_TO_HEX[item.difficulty_bg_class]
    }
    return undefined
  }

  function word(key: string | number, content: React.ReactNode, surface: string, base: string) {
    const color = colorFor(base, surface)
    return (
      <span
        key={key}
        role={onWordTap ? 'button' : undefined}
        onClick={onWordTap ? () => onWordTap(surface, base) : undefined}
        className={`rounded-md transition-colors ${onWordTap ? 'cursor-pointer hover:bg-blue-50 active:bg-amber-100' : ''}`}
        style={color ? { color, fontWeight: 600, textDecoration: 'underline', textDecorationColor: `${color}55`, textUnderlineOffset: 4 } : undefined}
      >
        {content}
      </span>
    )
  }

  function renderParagraph(p: string, pi: number) {
    const toks = tokens?.[pi]
    if (toks) {
      let offset = 0
      return toks.map((tok, i) => {
        const slice = sliceAnnotated(p, offset, tok.surface.length)
        offset += tok.surface.length
        const content = renderFurigana(slice, showFurigana)
        return isWordSegment(tok.surface)
          ? word(i, content, tok.surface, tok.base_form || tok.surface)
          : <span key={i}>{content}</span>
      })
    }
    const locale = text.language === 'ja' ? 'ja' : 'id'
    return parseFurigana(p).map((seg, si) => {
      if (seg.reading !== null) {
        const ruby = (
          <ruby className={showFurigana ? 'furigana-show' : 'furigana-hide'}>
            {seg.text}<rp>(</rp><rt>{seg.reading}</rt><rp>)</rp>
          </ruby>
        )
        return word(si, ruby, seg.text, seg.text)
      }
      return (
        <span key={si}>
          {splitTappable(seg.text, locale).map((w, wi) => (isWordSegment(w) ? word(wi, w, w, w) : <span key={wi}>{w}</span>))}
        </span>
      )
    })
  }

  return (
    <div className={`${SIZE_CLASS[size]} tracking-wide text-gray-800 space-y-4`}>
      {paragraphs.map((p, pi) => <p key={pi} className="m-0">{renderParagraph(p, pi)}</p>)}
    </div>
  )
}
