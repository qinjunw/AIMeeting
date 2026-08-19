import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import type { MeetingDetails } from '../../bridge/meetingRepositoryClient'
import { MeetingWorkspace } from './MeetingWorkspace'

const meeting: MeetingDetails = {
  id: 'meeting-1',
  title: '设计评审',
  recordingStatus: 'recording',
  transcriptionStatus: 'failed',
  minutesStatus: 'pending',
  createdAt: '2026-08-19T08:00:00Z',
  updatedAt: '2026-08-19T08:00:00Z',
  durationMs: 0,
  deletedAt: null,
  transcriptRevision: 0,
  transcript: '',
  minutes: null,
  audio: null,
}

describe('MeetingWorkspace', () => {
  it('uses the live recording status for the active meeting badge', () => {
    render(
      <MeetingWorkspace
        meeting={meeting}
        activeMeetingId="meeting-1"
        activeRecordingStatus="paused"
        transcription={{
          status: 'failed',
          error: null,
          revision: 0,
          interimText: '',
          segments: [],
        }}
        minutes={{
          status: 'pending',
          error: null,
          transcriptRevision: 0,
          content: '',
        }}
        tab="minutes"
        onTabChange={vi.fn()}
        onRetryTranscription={vi.fn()}
        onRetryMinutes={vi.fn()}
      />,
    )

    expect(screen.getByText('已暂停')).toHaveClass('meeting-status--paused')
  })
})
