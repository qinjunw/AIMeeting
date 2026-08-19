import { describe, expect, it } from 'vitest'
import { normalizeTranscriptText, toSimplifiedChinese } from './chineseText'

describe('Chinese transcript normalization', () => {
  it('converts traditional Chinese to simplified Chinese', () => {
    expect(toSimplifiedChinese('會議記錄與產品開發')).toBe('会议记录与产品开发')
  })

  it('collapses whitespace around recognized speech', () => {
    expect(normalizeTranscriptText('  我們\n正在   開會  ')).toBe('我们 正在 开会')
  })
})
