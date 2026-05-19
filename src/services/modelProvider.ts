import type { Evidence, MeetingSegment, ProviderConfig, SearchTrace } from '../types'
import { rankEvidence, getContextWindow } from './meetingMemory'
import { toSimplifiedChinese } from './chineseText'

type ChatMessage = {
  role: 'system' | 'user'
  content: string
}

type AgentDraft = {
  answer: string
  planItems: string[]
  evidence: Evidence[]
  providerLabel: string
  error?: string
}

type ProviderPayload = {
  answer?: string
  digest?: string
  planItems?: string[]
  plan_items?: string[]
}

type MeetingDigestDraft = {
  digest: string
  providerLabel: string
  error?: string
}

export async function generateAgentDraft(params: {
  question: string
  segments: MeetingSegment[]
  searches: SearchTrace[]
  provider: ProviderConfig
}): Promise<AgentDraft> {
  const fallback = buildLocalDraft(params.question, params.segments, params.searches)

  if (!params.provider.apiKey.trim() || !params.provider.baseUrl.trim() || !params.provider.model.trim()) {
    return {
      answer: '还没有配置会议问答模型。请填写 Provider 的 Base URL、Model 和 API key 后再提问。',
      planItems: [],
      evidence: fallback.evidence,
      providerLabel: 'provider not configured',
      error: '未配置 OpenAI-compatible 文本模型，本次没有生成硬编码草稿。',
    }
  }

  try {
    const messages = buildMessages(params.question, params.segments, params.searches)
    const text = await callProvider(params.provider, messages)
    const parsed = parseProviderPayload(text)

    return {
      answer: parsed.answer || text || fallback.answer,
      planItems: parsed.planItems ?? parsed.plan_items ?? fallback.planItems,
      evidence: fallback.evidence,
      providerLabel: `${params.provider.model} via ${params.provider.endpointFlavor}`,
    }
  } catch (error) {
    return {
      answer: '会议问答模型调用失败。请检查 Provider 配置、网络或模型服务状态后重试。',
      planItems: [],
      evidence: fallback.evidence,
      providerLabel: 'provider error',
      error: error instanceof Error ? error.message : 'Unknown provider error',
    }
  }
}

export async function generateMeetingDigest(params: {
  previousDigest: string
  newSegments: MeetingSegment[]
  provider: ProviderConfig
}): Promise<MeetingDigestDraft> {
  if (!params.provider.apiKey.trim() || !params.provider.baseUrl.trim() || !params.provider.model.trim()) {
    return {
      digest: params.previousDigest,
      providerLabel: 'provider not configured',
      error: '未配置 OpenAI-compatible 文本模型，递增纪要暂不生成。',
    }
  }

  try {
    const messages = buildDigestMessages(params.previousDigest, params.newSegments)
    const text = await callProvider(params.provider, messages)
    const parsed = parseProviderPayload(text)
    const digest = toSimplifiedChinese(parsed.digest || parsed.answer || text).trim()

    return {
      digest: digest || params.previousDigest,
      providerLabel: `${params.provider.model} via ${params.provider.endpointFlavor}`,
    }
  } catch (error) {
    return {
      digest: params.previousDigest,
      providerLabel: 'provider error',
      error: error instanceof Error ? error.message : 'Unknown provider error',
    }
  }
}

function buildMessages(question: string, segments: MeetingSegment[], searches: SearchTrace[]): ChatMessage[] {
  const context = getContextWindow(segments, 16)
  const searchTrail = searches
    .map((trace) => {
      const sources = trace.sources.map((source) => `${source.title}: ${source.url}`).join('; ')
      return `query="${trace.query}" status=${trace.status}${sources ? ` sources=${sources}` : ''}`
    })
    .join('\n')

  return [
    {
      role: 'system',
      content:
        'You are a meeting copilot. Answer in Chinese. Separate meeting facts, external search signals, and your own inference. Return compact JSON with keys answer and planItems.',
    },
    {
      role: 'user',
      content: [
        `User question: ${question}`,
        '',
        'Recent meeting transcript:',
        context || '(empty)',
        '',
        'Search trail:',
        searchTrail || '(none)',
      ].join('\n'),
    },
  ]
}

function buildDigestMessages(previousDigest: string, newSegments: MeetingSegment[]): ChatMessage[] {
  const newTranscript = getContextWindow(newSegments, newSegments.length)

  return [
    {
      role: 'system',
      content: [
        '你是实时会议纪要维护助手，只输出严格 JSON。',
        'JSON 结构为 {"digest":"..."}。',
        'digest 必须是简体中文会议纪要，不要输出原始逐字稿，不要记录对助手的操作指令。',
        '根据上一版纪要和新增转写，输出一份完整更新后的递增纪要。',
        '允许小幅重写、合并重复、修正明显错字，但不要删除已经确认的会议事实。',
      ].join('\n'),
    },
    {
      role: 'user',
      content: [
        '上一版会议纪要：',
        previousDigest || '(空)',
        '',
        '新增会议转写：',
        newTranscript || '(空)',
      ].join('\n'),
    },
  ]
}

async function callProvider(config: ProviderConfig, messages: ChatMessage[]): Promise<string> {
  const baseUrl = config.baseUrl.replace(/\/+$/, '')
  const endpoint = config.endpointFlavor === 'responses' ? `${baseUrl}/responses` : `${baseUrl}/chat/completions`

  const body =
    config.endpointFlavor === 'responses'
      ? {
          model: config.model,
          input: messages.map((message) => ({
            role: message.role,
            content: message.content,
          })),
          temperature: config.temperature,
        }
      : {
          model: config.model,
          messages,
          temperature: config.temperature,
          response_format: { type: 'json_object' },
        }

  const response = await fetch(endpoint, {
    method: 'POST',
    headers: {
      'content-type': 'application/json',
      authorization: `Bearer ${config.apiKey}`,
    },
    body: JSON.stringify(body),
  })

  if (!response.ok) {
    const detail = await response.text()
    throw new Error(`Provider returned ${response.status}: ${detail.slice(0, 240)}`)
  }

  const data = await response.json()
  return (
    data.output_text ??
    data.choices?.[0]?.message?.content ??
    data.output?.[0]?.content?.[0]?.text ??
    JSON.stringify(data)
  )
}

function parseProviderPayload(text: string): ProviderPayload {
  try {
    return JSON.parse(text) as ProviderPayload
  } catch {
    const start = text.indexOf('{')
    const end = text.lastIndexOf('}')

    if (start >= 0 && end > start) {
      try {
        return JSON.parse(text.slice(start, end + 1)) as ProviderPayload
      } catch {
        return { answer: text }
      }
    }

    return { answer: text }
  }
}

function buildLocalDraft(question: string, segments: MeetingSegment[], searches: SearchTrace[]): AgentDraft {
  const evidence = [
    ...rankEvidence(question, segments, 4),
    ...searches.flatMap((trace) =>
      trace.sources.slice(0, 2).map((source) => ({
        id: `ev_${trace.id}_${source.url}`,
        kind: 'web' as const,
        title: source.title,
        detail: source.snippet || trace.query,
        url: source.url,
      })),
    ),
  ]

  const recentFacts = evidence
    .filter((item) => item.kind === 'meeting')
    .slice(0, 3)
    .map((item) => item.detail)

  const searchStatus =
    searches.length === 0
      ? '本次没有外部搜索。'
      : searches.every((trace) => trace.status === 'planned')
        ? '已生成自动搜索 query，但还没有配置搜索 API endpoint。'
        : `已记录 ${searches.length} 条搜索轨迹。`

  return {
    answer: [
      `基于已记录会议，"${question}" 可以先按低风险原型验证，而不是直接做完整产品化。`,
      recentFacts.length > 0 ? `会议依据：${recentFacts.join(' ')}` : '会议依据还不足，需要继续记录更多上下文。',
      searchStatus,
      '推断：当前最值得优先验证的是会中上下文问答是否真的能减少参会者回忆和整理成本。',
    ].join('\n\n'),
    planItems: [
      '锁定一个 30-60 分钟中英混合会议样本，持续写入 transcript segment。',
      '用快捷键触发一次会中提问，检查回答是否引用到正确的最近片段。',
      '记录自动搜索 query 和来源，确认敏感信息是否需要脱敏。',
      '会后删除本地会议记录，验证数据保留策略可执行。',
    ],
    evidence,
    providerLabel: 'local deterministic draft',
  }
}
