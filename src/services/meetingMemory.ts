import type { Evidence, MeetingSegment } from '../types'
import { formatDuration } from '../lib/time'

const stopWords = new Set([
  'the',
  'and',
  'for',
  'with',
  'that',
  'this',
  '刚才',
  '我们',
  '这个',
  '一下',
  '可以',
  '有没有',
  '如何',
  '什么',
])

export function buildRollingSummary(segments: MeetingSegment[]): string {
  if (segments.length === 0) {
    return '还没有会议上下文。'
  }

  const latest = segments.slice(-5).map((segment) => segment.text)
  const topics = extractKeywords(latest.join(' '), 5)

  return [
    `当前已记录 ${segments.length} 个片段，覆盖 ${formatDuration(segments.at(-1)?.endMs ?? 0)}。`,
    topics.length > 0 ? `高频主题：${topics.join('、')}。` : '高频主题还不明显。',
    '当前 MVP 关注：会中上下文问答、计划生成、搜索日志和可展开证据。',
  ].join(' ')
}

export function getContextWindow(segments: MeetingSegment[], limit = 12): string {
  return segments
    .slice(-limit)
    .map((segment) => {
      const stamp = `${formatDuration(segment.startMs)}-${formatDuration(segment.endMs)}`
      return `[${stamp}] ${segment.speakerLabel}: ${segment.text}`
    })
    .join('\n')
}

export function rankEvidence(question: string, segments: MeetingSegment[], maxItems = 4): Evidence[] {
  const keywords = extractKeywords(question, 6)
  const scored = segments
    .map((segment) => {
      const normalized = segment.text.toLowerCase()
      const score = keywords.reduce((sum, keyword) => sum + (normalized.includes(keyword.toLowerCase()) ? 2 : 0), 0)
      const recency = segment.endMs / Math.max(1, segments.at(-1)?.endMs ?? 1)
      return { segment, score: score + recency }
    })
    .sort((a, b) => b.score - a.score)

  return scored.slice(0, maxItems).map(({ segment }) => ({
    id: `ev_${segment.id}`,
    kind: 'meeting',
    title: `${segment.speakerLabel} ${formatDuration(segment.startMs)}`,
    detail: segment.text,
    segmentId: segment.id,
    confidence: segment.confidence,
  }))
}

export function extractKeywords(text: string, maxItems = 6): string[] {
  const words = text
    .replace(/[^\p{L}\p{N}\s-]/gu, ' ')
    .split(/\s+/)
    .map((word) => word.trim())
    .filter((word) => word.length >= 2 && !stopWords.has(word.toLowerCase()))

  const counts = new Map<string, number>()
  for (const word of words) {
    counts.set(word, (counts.get(word) ?? 0) + 1)
  }

  return [...counts.entries()]
    .sort((a, b) => b[1] - a[1])
    .slice(0, maxItems)
    .map(([word]) => word)
}
