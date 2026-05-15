import { useState } from 'react'
import { CloseOutlined } from '@ant-design/icons'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'

export type UserFeedback = { messageId: number; score: number; feedback: string; loading: boolean }

type Props = {
  feedback: UserFeedback | undefined
}

type VocabItem = { word: string; reading_meaning: string; ok: boolean; note: string }
type GrammarItem = { pattern: string; ok: boolean; note: string }
type TargetWordItem = { word: string; used: boolean; note: string }
type FeedbackJSON = {
  score: number
  summary: string
  vocab?: VocabItem[]
  grammar?: GrammarItem[]
  target_words?: TargetWordItem[]
  suggestion?: string
}

function scoreColor(score: number): string {
  if (score >= 4) return 'text-green-600 bg-green-50'
  if (score >= 3) return 'text-yellow-600 bg-yellow-50'
  return 'text-orange-500 bg-orange-50'
}

function stars(score: number): string {
  return '★'.repeat(score) + '☆'.repeat(5 - score)
}

/** Try to parse feedback as JSON; return null if it's old-style markdown */
function tryParseJSON(raw: string): FeedbackJSON | null {
  const cleaned = raw.trim().replace(/^```json\s*\n?/i, '').replace(/\n?```\s*$/i, '').trim()
  try {
    const obj = JSON.parse(cleaned)
    if (typeof obj.score === 'number' && typeof obj.summary === 'string') return obj
  } catch {}
  return null
}

/** Convert structured JSON feedback to markdown for rendering */
function feedbackToMarkdown(fb: FeedbackJSON): string {
  const lines: string[] = []

  lines.push(`**Overall Score**`)
  lines.push(`[ \`${stars(fb.score)}\` ] **${fb.score} / 5**`)
  lines.push('')
  lines.push(`> ${fb.summary}`)
  lines.push('')
  lines.push('---')
  lines.push('')

  if (fb.vocab && fb.vocab.length > 0) {
    lines.push('### 🔤 Vocabulary Analysis')
    lines.push('')
    lines.push('| Status | Word | Reading / Meaning | Diagnosis & Suggestion |')
    lines.push('|:---:|:---|:---|:---|')
    for (const v of fb.vocab) {
      const status = v.ok ? '✅' : '❌'
      lines.push(`| ${status} | **${v.word}** | *${v.reading_meaning}* | ${v.note} |`)
    }
    lines.push('')
    lines.push('---')
    lines.push('')
  }

  if (fb.grammar && fb.grammar.length > 0) {
    lines.push('### 📝 Grammar')
    lines.push('')
    lines.push('| Status | Grammar | Diagnosis |')
    lines.push('|:---:|:---|:---|')
    for (const g of fb.grammar) {
      const status = g.ok ? '✅' : '❌'
      lines.push(`| ${status} | **${g.pattern}** | ${g.note} |`)
    }
    lines.push('')
    lines.push('---')
    lines.push('')
  }

  if (fb.target_words && fb.target_words.length > 0) {
    lines.push('### 🎯 Target Word Usage')
    lines.push('')
    lines.push('| Status | Target Word | Used? | Notes |')
    lines.push('|:---:|:---|:---:|:---|')
    for (const tw of fb.target_words) {
      const status = tw.used ? '✅' : '❌'
      lines.push(`| ${status} | **${tw.word}** | ${tw.used ? 'Yes' : 'No'} | ${tw.note} |`)
    }
    lines.push('')
    lines.push('---')
    lines.push('')
  }

  if (fb.suggestion) {
    lines.push('### 📝 Suggestion')
    lines.push('')
    lines.push(`> ✅ **${fb.suggestion}**`)
    lines.push('')
  }

  return lines.join('\n')
}

export default function UserMessageFeedback({ feedback }: Props) {
  const [showDetail, setShowDetail] = useState(false)

  if (!feedback) return null

  if (feedback.loading) {
    return (
      <div className="mt-1 px-2 py-0.5 rounded-full bg-gray-100 text-[11px] text-gray-400 animate-pulse inline-flex items-center gap-1">
        <span className="w-1.5 h-1.5 rounded-full bg-gray-300" />
        Evaluating...
      </div>
    )
  }

  // Try JSON format first, fall back to raw markdown for cached entries
  const json = tryParseJSON(feedback.feedback)
  const displayMarkdown = json ? feedbackToMarkdown(json) : feedback.feedback

  return (
    <>
      <button
        onClick={() => setShowDetail(true)}
        className={`mt-1 inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-[11px] font-semibold transition-colors ${scoreColor(feedback.score)} hover:opacity-80 active:opacity-60`}
      >
        {feedback.score}/5
      </button>

      {showDetail && (
        <div
          className="fixed inset-0 z-50 flex items-end justify-center bg-black/30 sm:items-center"
          onClick={() => setShowDetail(false)}
        >
          <div
            className="bg-white rounded-t-2xl sm:rounded-2xl w-full max-w-lg max-h-[70vh] flex flex-col safe-bottom"
            onClick={e => e.stopPropagation()}
          >
            <div className="flex items-center justify-between px-4 py-3 border-b shrink-0">
              <div className="flex items-center gap-2">
                <span className={`text-sm font-bold ${feedback.score >= 4 ? 'text-green-600' : feedback.score >= 3 ? 'text-yellow-600' : 'text-orange-500'}`}>
                  {feedback.score}/5
                </span>
                <span className="text-sm font-medium text-gray-700">Message Feedback</span>
              </div>
              <button
                onClick={() => setShowDetail(false)}
                className="w-8 h-8 flex items-center justify-center rounded-full hover:bg-gray-100 active:bg-gray-200"
              >
                <CloseOutlined />
              </button>
            </div>
            <div className="flex-1 overflow-y-auto px-4 py-3 prose prose-sm max-w-none text-gray-700
              [&_h2]:text-sm [&_h2]:font-semibold [&_h2]:mb-2 [&_h2]:mt-3
              [&_h3]:text-[13px] [&_h3]:font-semibold [&_h3]:mt-3 [&_h3]:mb-1.5
              [&_ul]:my-1 [&_li]:text-[13px]
              [&_table]:w-full [&_table]:text-xs [&_table]:border-collapse
              [&_th]:bg-gray-50 [&_th]:px-2 [&_th]:py-1.5 [&_th]:text-left [&_th]:font-semibold [&_th]:border-b
              [&_td]:px-2 [&_td]:py-1.5 [&_td]:border-b
              [&_tr]:hover:bg-gray-50
              [&_blockquote]:border-l-4 [&_blockquote]:border-indigo-300 [&_blockquote]:pl-3 [&_blockquote]:py-1 [&_blockquote]:my-2 [&_blockquote]:bg-indigo-50/50 [&_blockquote]:rounded-r [&_blockquote]:text-[13px]
              [&_blockquote_p]:m-0
              [&_hr]:my-3 [&_hr]:border-gray-200
              [&_strong]:font-semibold [&_code]:bg-gray-100 [&_code]:px-1 [&_code]:py-0.5 [&_code]:rounded [&_code]:text-[12px] [&_em]:text-gray-600
            ">
              <ReactMarkdown remarkPlugins={[remarkGfm]}>{displayMarkdown}</ReactMarkdown>
            </div>
          </div>
        </div>
      )}
    </>
  )
}
