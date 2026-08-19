// @vitest-environment jsdom

import { StrictMode } from 'react'
import { act, renderHook } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import type {
  MeetingSessionEvent,
  RecordingClient,
  SessionSnapshot,
} from '../../bridge/recordingClient'
import type { ProcessingJobClient } from '../../bridge/providerClient'
import { useMeetingSession } from './useMeetingSession'

const initialSnapshot: SessionSnapshot = {
  meetingId: 'meeting-1',
  runGeneration: 1,
  recordingStatus: 'recording',
  transcriptRevision: 0,
  transcriptionStatus: 'streaming',
  transcriptionError: null,
  minutesStatus: 'pending',
  minutesError: null,
}

function createHarness() {
  const listeners = new Map<number, (event: MeetingSessionEvent) => void>()
  const jobListeners = new Map<
    number,
    Parameters<ProcessingJobClient['subscribe']>[0]
  >()
  let nextListenerId = 0
  let nextJobListenerId = 0
  const unlisten = vi.fn()
  const jobUnlisten = vi.fn()
  const client: RecordingClient = {
    start: vi.fn().mockResolvedValue(initialSnapshot),
    pause: vi.fn(),
    resume: vi.fn(),
    stop: vi.fn(),
    getActive: vi.fn().mockResolvedValue(null),
    subscribe: vi.fn(async (listener) => {
      const listenerId = ++nextListenerId
      listeners.set(listenerId, listener)
      return () => {
        listeners.delete(listenerId)
        unlisten()
      }
    }),
  }
  const jobs: ProcessingJobClient = {
    list: vi.fn().mockResolvedValue([]),
    retryTranscription: vi.fn(),
    retryMinutes: vi.fn(),
    subscribe: vi.fn(async (listener) => {
      const listenerId = ++nextJobListenerId
      jobListeners.set(listenerId, listener)
      return () => {
        jobListeners.delete(listenerId)
        jobUnlisten()
      }
    }),
  }

  return {
    client,
    jobs,
    listeners,
    jobListeners,
    unlisten,
    jobUnlisten,
    emit(event: MeetingSessionEvent) {
      listeners.forEach((listener) => listener(event))
    },
  }
}

describe('useMeetingSession', () => {
  it('keeps one effective subscription in StrictMode and unlistens on unmount', async () => {
    const harness = createHarness()
    const view = renderHook(
      () =>
        useMeetingSession({
          recordingClient: harness.client,
          processingJobClient: harness.jobs,
        }),
      { wrapper: StrictMode },
    )

    await act(async () => Promise.resolve())
    expect(harness.listeners.size).toBe(1)
    expect(harness.jobListeners.size).toBe(1)

    view.unmount()
    await act(async () => Promise.resolve())
    expect(harness.listeners.size).toBe(0)
    expect(harness.jobListeners.size).toBe(0)
    expect(harness.unlisten).toHaveBeenCalledTimes(2)
    expect(harness.jobUnlisten).toHaveBeenCalledTimes(2)
  })

  it('separates recording, transcription and minutes while rejecting late events', async () => {
    const harness = createHarness()
    const { result } = renderHook(() =>
      useMeetingSession({
        recordingClient: harness.client,
        processingJobClient: harness.jobs,
      }),
    )
    await act(async () => Promise.resolve())

    await act(async () => {
      await result.current.start({
        title: '设计评审',
        sources: { microphone: true, systemAudio: true },
      })
    })

    act(() => {
      harness.emit({
        kind: 'transcription-status',
        meetingId: 'meeting-1',
        runGeneration: 1,
        revision: 1,
        status: 'failed',
        error: 'ASR 暂时不可用',
      })
    })
    expect(result.current.recording.status).toBe('recording')
    expect(result.current.transcription.status).toBe('failed')
    expect(result.current.minutes.status).toBe('pending')

    act(() => {
      harness.emit({
        kind: 'transcript-interim',
        meetingId: 'meeting-1',
        runGeneration: 1,
        revision: 2,
        text: '讨论发布计划',
      })
      harness.emit({
        kind: 'transcript-final',
        meetingId: 'meeting-1',
        runGeneration: 1,
        revision: 2,
        segmentId: 'segment-1',
        text: '讨论发布计划。',
        beginMs: 0,
        endMs: 1600,
      })
      harness.emit({
        kind: 'transcript-final',
        meetingId: 'meeting-1',
        runGeneration: 1,
        revision: 1,
        segmentId: 'late-revision',
        text: '旧版本',
        beginMs: 0,
        endMs: 300,
      })
      harness.emit({
        kind: 'transcript-final',
        meetingId: 'another-meeting',
        runGeneration: 1,
        revision: 3,
        segmentId: 'wrong-meeting',
        text: '串会内容',
        beginMs: 0,
        endMs: 300,
      })
      harness.emit({
        kind: 'transcript-final',
        meetingId: 'meeting-1',
        runGeneration: 0,
        revision: 3,
        segmentId: 'old-run',
        text: '旧录音轮次',
        beginMs: 0,
        endMs: 300,
      })
    })

    expect(result.current.transcription.revision).toBe(2)
    expect(result.current.transcription.interimText).toBe('')
    expect(result.current.transcription.segments.map((segment) => segment.id)).toEqual([
      'segment-1',
    ])

    act(() => {
      harness.emit({
        kind: 'minutes-status',
        meetingId: 'meeting-1',
        runGeneration: 1,
        transcriptRevision: 2,
        status: 'ready',
        content: '发布计划已确认。',
        error: null,
      })
      harness.emit({
        kind: 'minutes-status',
        meetingId: 'meeting-1',
        runGeneration: 1,
        transcriptRevision: 1,
        status: 'ready',
        content: '过期纪要',
        error: null,
      })
    })

    expect(result.current.minutes.transcriptRevision).toBe(2)
    expect(result.current.minutes.content).toBe('发布计划已确认。')
  })
})
