import { invoke } from '@tauri-apps/api/core'
import type { AsrProviderConfig, AsrRuntimeStatus, AsrTranscriptionResponse } from '../types'

export async function getAsrRuntimeStatus(): Promise<AsrRuntimeStatus> {
  if (!isTauriRuntime()) {
    return { localReady: false, runtimeLabel: 'whisper.cpp small + Silero VAD' }
  }

  return invoke<AsrRuntimeStatus>('asr_runtime_status')
}

export async function transcribeAudioChunk(params: {
  audio: Blob
  provider: AsrProviderConfig
  language: string
}): Promise<AsrTranscriptionResponse> {
  if (!isTauriRuntime()) {
    throw new Error('本地/云端 ASR 转写需要在 Tauri 桌面版中运行。')
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

function isTauriRuntime(): boolean {
  const target = window as Window & {
    __TAURI__?: unknown
    __TAURI_INTERNALS__?: unknown
  }

  return Boolean(target.__TAURI__ || target.__TAURI_INTERNALS__)
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
