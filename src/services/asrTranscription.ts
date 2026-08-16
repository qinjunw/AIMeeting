import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type {
  AsrProviderConfig,
  AsrTranscriptionResponse,
  StreamingAsrEvent,
  StreamingAsrSessionResponse,
} from '../types'

export async function transcribeAudioChunk(params: {
  audio: Blob
  provider: AsrProviderConfig
  language: string
}): Promise<AsrTranscriptionResponse> {
  if (!isTauriRuntime()) {
    throw new Error('云端 ASR 转写需要在 Tauri 桌面版中运行。')
  }

  const audioBase64 = await blobToBase64(params.audio)

  return invoke<AsrTranscriptionResponse>('transcribe_audio_chunk', {
    request: {
      audioBase64,
      mimeType: params.audio.type || 'audio/webm',
      cloudBaseUrl: params.provider.baseUrl,
      cloudApiKey: params.provider.apiKey,
      cloudModel: params.provider.model,
      language: params.language,
    },
  })
}

export async function startStreamingAsrSession(params: {
  provider: AsrProviderConfig
  language: string
  meetingId: string
  recordingRunId: string
}): Promise<StreamingAsrSessionResponse> {
  ensureTauriRuntime()

  return invoke<StreamingAsrSessionResponse>('start_streaming_asr_session', {
    request: {
      cloudBaseUrl: params.provider.baseUrl,
      cloudApiKey: params.provider.apiKey,
      cloudModel: params.provider.model,
      language: params.language,
      meetingId: params.meetingId,
      recordingRunId: params.recordingRunId,
    },
  })
}

export async function pushStreamingAsrAudio(params: { sessionId: string; audioBase64: string }): Promise<void> {
  ensureTauriRuntime()

  return invoke('push_streaming_asr_audio', {
    request: {
      sessionId: params.sessionId,
      audioBase64: params.audioBase64,
    },
  })
}

export async function finishStreamingAsrSession(sessionId: string): Promise<void> {
  ensureTauriRuntime()

  return invoke('finish_streaming_asr_session', {
    request: {
      sessionId,
    },
  })
}

export async function cancelStreamingAsrSession(sessionId: string): Promise<void> {
  ensureTauriRuntime()

  return invoke('cancel_streaming_asr_session', {
    request: {
      sessionId,
    },
  })
}

export function listenStreamingAsrEvents(handler: (event: StreamingAsrEvent) => void): Promise<UnlistenFn> {
  ensureTauriRuntime()

  return listen<StreamingAsrEvent>('streaming-asr-event', (event) => handler(event.payload))
}

function isTauriRuntime(): boolean {
  const target = window as Window & {
    __TAURI__?: unknown
    __TAURI_INTERNALS__?: unknown
  }

  return Boolean(target.__TAURI__ || target.__TAURI_INTERNALS__)
}

function ensureTauriRuntime() {
  if (!isTauriRuntime()) {
    throw new Error('云端 ASR 转写需要在 Tauri 桌面版中运行。')
  }
}

function blobToBase64(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()

    reader.onerror = () => reject(new Error('Unable to read audio chunk.'))
    reader.onload = () => {
      const result = String(reader.result ?? '')
      const [, base64 = ''] = result.split(',')
      resolve(base64)
    }
    reader.readAsDataURL(blob)
  })
}
