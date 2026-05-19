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

export async function probeSystemAudio(): Promise<CaptureProbe> {
  try {
    const stream = await navigator.mediaDevices.getDisplayMedia({
      video: true,
      audio: true,
    })
    const audioTracks = stream.getAudioTracks()
    const videoTracks = stream.getVideoTracks()
    const label = audioTracks[0]?.label || videoTracks[0]?.label || 'Display capture granted'
    stopStream(stream)

    return {
      ok: audioTracks.length > 0,
      label,
      detail:
        audioTracks.length > 0
          ? '浏览器授予了显示/系统音频轨道。Windows 桌面版后续应走 WASAPI loopback。'
          : '浏览器只授予了屏幕轨道，没有系统音频；桌面版需要原生 WASAPI。',
    }
  } catch (error) {
    return {
      ok: false,
      label: 'System audio probe failed',
      detail: error instanceof Error ? error.message : '无法请求系统音频探测。',
    }
  }
}

function stopStream(stream: MediaStream): void {
  for (const track of stream.getTracks()) {
    track.stop()
  }
}
