// @vitest-environment jsdom

import { act, renderHook, waitFor } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import type {
  MeetingDetails,
  MeetingRepositoryClient,
  MeetingSummary,
} from '../../bridge/meetingRepositoryClient'
import { useMeetingHistory } from './useMeetingHistory'

const summary: MeetingSummary = {
  id: 'meeting-1',
  title: '设计评审',
  recordingStatus: 'ready',
  transcriptionStatus: 'ready',
  minutesStatus: 'ready',
  createdAt: '2026-08-19T08:00:00Z',
  updatedAt: '2026-08-19T09:00:00Z',
  durationMs: 3_600_000,
  deletedAt: null,
}

const details: MeetingDetails = {
  ...summary,
  transcriptRevision: 3,
  transcript: '评审结论：通过。',
  minutes: {
    transcriptRevision: 3,
    content: '评审通过。',
    providerLabel: 'qwen',
  },
  audio: {
    relativePath: 'meetings/meeting-1/recording.ogg',
    playbackPath: 'C:\\recording.opus',
    format: 'ogg_opus',
    status: 'ready',
    durationMs: 3_600_000,
    byteSize: 1024,
  },
}

describe('useMeetingHistory', () => {
  it('loads history once, resolves selection and removes trashed meetings', async () => {
    const client: MeetingRepositoryClient = {
      list: vi.fn().mockResolvedValue({ items: [summary], nextCursor: null }),
      get: vi.fn().mockResolvedValue(details),
      rename: vi.fn(),
      moveToTrash: vi.fn().mockResolvedValue(undefined),
      restore: vi.fn(),
      permanentlyDelete: vi.fn(),
      emptyTrash: vi.fn(),
    }
    const { result } = renderHook(() => useMeetingHistory(client))

    await waitFor(() => expect(result.current.status).toBe('ready'))
    expect(result.current.meetings).toEqual([summary])

    await act(async () => result.current.selectMeeting('meeting-1'))
    expect(result.current.selectedMeeting).toEqual(details)

    await act(async () => result.current.moveToTrash('meeting-1'))
    expect(result.current.meetings).toEqual([])
    expect(result.current.selectedMeetingId).toBeNull()
    expect(result.current.selectedMeeting).toBeNull()
  })
})
