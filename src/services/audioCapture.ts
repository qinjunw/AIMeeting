import type { CaptureProbe } from '../types'

export async function probeMicrophone(): Promise<CaptureProbe> {
  try {
    const stream = await navigator.mediaDevices.getUserMedia({ audio: true })
    const tracks = stream.getAudioTracks()
    const label = tracks[0]?.label || 'Microphone granted'
    stopStream(stream)

    return {
      ok: true,
      label,
      detail: '麦克风权限可用。实时转写会使用已配置的云端 ASR Provider。',
    }
  } catch (error) {
    return {
      ok: false,
      label: 'Microphone unavailable',
      detail: error instanceof Error ? error.message : '无法请求麦克风权限。',
    }
  }
}

function stopStream(stream: MediaStream): void {
  for (const track of stream.getTracks()) {
    track.stop()
  }
}
