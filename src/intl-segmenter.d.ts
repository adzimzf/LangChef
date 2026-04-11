// Intl.Segmenter type declarations (available in modern browsers/WebKit but not in TS yet)
declare namespace Intl {
  interface Segmenter {
    segment(text: string): IterableIterator<SegmentData>
  }

  interface SegmentData {
    segment: string
    index: number
    input: string
    isWordLike: boolean
  }

  interface SegmenterOptions {
    granularity?: 'grapheme' | 'word' | 'sentence'
    locale?: string
  }

  var Segmenter: {
    new (locale?: string, options?: SegmenterOptions): Segmenter
    supportedLocalesOf(locales: string | string[], options?: SegmenterOptions): string[]
  }
}
