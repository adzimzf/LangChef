import { useMemo } from 'react'
import { parseFurigana } from '../../lib/furigana'
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

interface Props {
  text: ReadingTextType
  reviewItems: ReviewItem[]
  showFurigana: boolean
}

export default function ReadingText({ text, reviewItems, showFurigana }: Props) {
  const reviewMap = useMemo(() => {
    const map = new Map<string, ReviewItem>()
    for (const item of reviewItems) {
      if (!item.language || item.language === text.language) {
        map.set(item.content, item)
        map.set(item.content.toLowerCase(), item)
      }
    }
    return map
  }, [reviewItems, text.language])

  function colorForWord(word: string): string | undefined {
    const item = reviewMap.get(word) ?? reviewMap.get(word.toLowerCase())
    if (!item) return undefined
    return BG_TO_HEX[item.difficulty_bg_class]
  }

  const segments = parseFurigana(text.body)
  const locale = text.language === 'ja' ? 'ja' : 'id'

  return (
    <div className="text-[17px] leading-[2] tracking-wide">
      {segments.map((seg, si) => {
        if (seg.reading !== null) {
          const color = colorForWord(seg.text)
          return (
            <ruby
              key={si}
              className={showFurigana ? 'furigana-show' : 'furigana-hide'}
              style={color ? { color, fontWeight: 600 } : undefined}
            >
              {seg.text}
              <rp>(</rp><rt>{seg.reading}</rt><rp>)</rp>
            </ruby>
          )
        }
        const words = splitTappable(seg.text, locale)
        return (
          <span key={si}>
            {words.map((word, wi) => {
              if (!isWordSegment(word)) return <span key={wi}>{word}</span>
              const color = colorForWord(word)
              return color
                ? <span key={wi} style={{ color, fontWeight: 600 }}>{word}</span>
                : <span key={wi}>{word}</span>
            })}
          </span>
        )
      })}
    </div>
  )
}
