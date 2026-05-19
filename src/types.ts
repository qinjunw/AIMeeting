export type MeetingMode = 'recording' | 'wake-beta' | 'dialogue' | 'searching' | 'paused'

export type AudioSource = 'system' | 'microphone' | 'mixed'

export type SegmentStatus = 'draft' | 'final'

export type MeetingSegment = {
  id: string
  speakerLabel: string
  source: AudioSource
  startMs: number
  endMs: number
  text: string
  confidence: number
  status: SegmentStatus
  createdAt: string
}

export type MeetingDigest = {
  text: string
  updatedAt: string
  providerLabel: string
  segmentCount: number
  error?: string
}

export type ProviderEndpointFlavor = 'chat-completions' | 'responses'

export type ProviderConfig = {
  baseUrl: string
  apiKey: string
  model: string
  endpointFlavor: ProviderEndpointFlavor
  temperature: number
}

export type AsrProviderConfig = {
  baseUrl: string
  apiKey: string
  model: string
}

export type AsrRuntimeStatus = {
  whisperServerPath?: string
  modelPath?: string
  vadModelPath?: string
  localServerUrl?: string
  localReady: boolean
  runtimeLabel: string
}

export type AsrTranscriptionResponse = {
  text: string
  providerLabel: string
  usedFallback: boolean
  warning?: string
  localServerUrl?: string
}

export type SearchMode = 'auto' | 'confirm' | 'off'

export type SearchConfig = {
  mode: SearchMode
  endpointTemplate: string
  redactBeforeSearch: boolean
}

export type SearchSource = {
  title: string
  url: string
  snippet: string
}

export type SearchTrace = {
  id: string
  query: string
  status: 'planned' | 'completed' | 'failed' | 'skipped'
  createdAt: string
  sources: SearchSource[]
  error?: string
}

export type Evidence = {
  id: string
  kind: 'meeting' | 'web' | 'inference'
  title: string
  detail: string
  segmentId?: string
  url?: string
  confidence?: number
}

export type AgentResponse = {
  id: string
  question: string
  answer: string
  planItems: string[]
  evidence: Evidence[]
  searches: SearchTrace[]
  providerLabel: string
  latencyMs: number
  createdAt: string
  error?: string
}

export type CaptureProbe = {
  ok: boolean
  label: string
  detail: string
}

export type SpeechRecognitionSupport = {
  supported: boolean
  label: string
  detail: string
}

export type SpeechRecognitionStatus = 'idle' | 'listening' | 'stopping' | 'error' | 'unsupported'

export type VoiceTrigger = {
  phrase: string
  transcript: string
  question: string
  beforeText: string
}
