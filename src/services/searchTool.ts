import type { MeetingSegment, SearchConfig, SearchSource, SearchTrace } from '../types'
import { makeId } from '../lib/time'
import { extractKeywords } from './meetingMemory'

type GenericSearchResponse = {
  results?: SearchSource[]
  webPages?: {
    value?: Array<{
      name?: string
      url?: string
      snippet?: string
    }>
  }
}

export function buildSearchQueries(question: string, segments: MeetingSegment[]): string[] {
  const meetingTerms = extractKeywords(segments.slice(-8).map((segment) => segment.text).join(' '), 4)
  const questionTerms = extractKeywords(question, 4)
  const primary = [...questionTerms, ...meetingTerms].slice(0, 8).join(' ')

  if (!primary) {
    return []
  }

  return [`${primary} feasibility`, `${primary} implementation plan`]
}

export async function runAutoSearch(
  question: string,
  segments: MeetingSegment[],
  config: SearchConfig,
): Promise<SearchTrace[]> {
  if (config.mode === 'off') {
    return []
  }

  if (config.mode === 'confirm') {
    return buildSearchQueries(question, segments).slice(0, 1).map((query) => ({
      id: makeId('search'),
      query: maybeRedact(query, config.redactBeforeSearch),
      status: 'skipped',
      createdAt: new Date().toISOString(),
      sources: [],
      error: '搜索策略为确认后执行；当前原型先记录候选 query。',
    }))
  }

  const queries = buildSearchQueries(question, segments).slice(0, 2)
  const traces: SearchTrace[] = []

  for (const rawQuery of queries) {
    const query = maybeRedact(rawQuery, config.redactBeforeSearch)

    if (!config.endpointTemplate.trim()) {
      traces.push({
        id: makeId('search'),
        query,
        status: 'planned',
        createdAt: new Date().toISOString(),
        sources: [],
        error: '尚未配置搜索 API endpoint，已记录自动搜索 query。',
      })
      continue
    }

    try {
      const url = buildEndpoint(config.endpointTemplate, query)
      const response = await fetch(url)

      if (!response.ok) {
        throw new Error(`Search endpoint returned ${response.status}`)
      }

      const payload = (await response.json()) as GenericSearchResponse
      traces.push({
        id: makeId('search'),
        query,
        status: 'completed',
        createdAt: new Date().toISOString(),
        sources: normalizeSources(payload),
      })
    } catch (error) {
      traces.push({
        id: makeId('search'),
        query,
        status: 'failed',
        createdAt: new Date().toISOString(),
        sources: [],
        error: error instanceof Error ? error.message : 'Unknown search error',
      })
    }
  }

  return traces
}

function buildEndpoint(template: string, query: string): string {
  const encoded = encodeURIComponent(query)
  return template.includes('{query}') ? template.replaceAll('{query}', encoded) : `${template}${template.includes('?') ? '&' : '?'}q=${encoded}`
}

function normalizeSources(payload: GenericSearchResponse): SearchSource[] {
  if (Array.isArray(payload.results)) {
    return payload.results.slice(0, 4)
  }

  return (payload.webPages?.value ?? []).slice(0, 4).map((item) => ({
    title: item.name ?? 'Untitled source',
    url: item.url ?? '#',
    snippet: item.snippet ?? '',
  }))
}

function maybeRedact(query: string, enabled: boolean): string {
  if (!enabled) {
    return query
  }

  return query
    .replace(/\b[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}\b/gi, '[email]')
    .replace(/\b(?:sk|pk|rk)-[A-Za-z0-9_-]{12,}\b/g, '[key]')
    .replace(/\b\d{4}[-\s]?\d{4}[-\s]?\d{4,}\b/g, '[number]')
}
