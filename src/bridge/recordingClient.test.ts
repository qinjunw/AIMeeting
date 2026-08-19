import { describe, expect, it, vi } from 'vitest'

import {
  createRecordingClient,
  type MeetingSessionEvent,
} from './recordingClient'
import type { DesktopTransport, Unsubscribe } from './transport'

describe('recordingClient', () => {
  it('maps lifecycle operations to typed Tauri commands', async () => {
    const invoke = vi.fn().mockResolvedValue({
      meetingId: 'meeting-1',
      runGeneration: 1,
      recordingStatus: 'recording',
      transcriptRevision: 0,
      transcriptionStatus: 'pending',
      transcriptionError: null,
      minutesStatus: 'pending',
      minutesError: null,
    })
    const transport: DesktopTransport = {
      invoke,
      listen: vi.fn(),
    }
    const client = createRecordingClient(transport)

    await client.start({
      title: '设计评审',
      sources: { microphone: true, systemAudio: true },
    })
    await client.pause({ meetingId: 'meeting-1', runGeneration: 1 })
    await client.resume({
      meetingId: 'meeting-1',
      runGeneration: 1,
      sources: { microphone: true, systemAudio: true },
    })
    await client.stop({ meetingId: 'meeting-1', runGeneration: 2 })

    expect(invoke).toHaveBeenNthCalledWith(1, 'start_recording', {
      request: {
        title: '设计评审',
        sources: { microphone: true, systemAudio: true },
      },
    })
    expect(invoke).toHaveBeenNthCalledWith(2, 'pause_recording', {
      request: { meetingId: 'meeting-1', runGeneration: 1 },
    })
    expect(invoke).toHaveBeenNthCalledWith(3, 'resume_recording', {
      request: {
        meetingId: 'meeting-1',
        runGeneration: 1,
        sources: { microphone: true, systemAudio: true },
      },
    })
    expect(invoke).toHaveBeenNthCalledWith(4, 'stop_recording', {
      request: { meetingId: 'meeting-1', runGeneration: 2 },
    })
  })

  it('combines typed desktop events and tears every listener down', async () => {
    const handlers = new Map<string, (payload: unknown) => void>()
    const unlisteners: Unsubscribe[] = []
    const listen = vi.fn(async <T>(
      eventName: string,
      handler: (payload: T) => void,
    ) => {
      handlers.set(eventName, handler as (payload: unknown) => void)
      const unlisten = vi.fn(() => handlers.delete(eventName))
      unlisteners.push(unlisten)
      return unlisten
    })
    const client = createRecordingClient({ invoke: vi.fn(), listen })
    const events: MeetingSessionEvent[] = []

    const unsubscribe = await client.subscribe((event) => events.push(event))
    handlers.get('meeting-state-event')?.({
      meetingId: 'meeting-1',
      runGeneration: 1,
      recordingStatus: 'recording',
    })
    handlers.get('transcription-event')?.({
      event: 'interim',
      meetingId: 'meeting-1',
      runGeneration: 1,
      revision: 2,
      text: '正在讨论',
    })
    handlers.get('minutes-event')?.({
      meetingId: 'meeting-1',
      runGeneration: 1,
      transcriptRevision: 2,
      status: 'ready',
      content: '纪要内容',
      error: null,
    })

    expect(events.map((event) => event.kind)).toEqual([
      'meeting-state',
      'transcript-interim',
      'minutes-status',
    ])

    unsubscribe()
    expect(unlisteners).toHaveLength(3)
    expect(unlisteners.every((unlisten) => vi.mocked(unlisten).mock.calls.length === 1)).toBe(true)
    expect(handlers.size).toBe(0)
  })
})
