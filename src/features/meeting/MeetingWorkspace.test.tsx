import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'

import type { MeetingDetails } from '../../bridge/meetingRepositoryClient'
import { MeetingWorkspace } from './MeetingWorkspace'

vi.mock('@tauri-apps/api/core', () => ({
  convertFileSrc: (path: string) => `asset://localhost/${encodeURIComponent(path)}`,
}))

afterEach(cleanup)

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

  it('opens a native audio player for a saved recording without requiring AI output', async () => {
    const user = userEvent.setup()
    const savedMeeting: MeetingDetails = {
      ...meeting,
      recordingStatus: 'ready',
      audio: {
        relativePath: 'recording.opus',
        playbackPath: 'C:\\Users\\tester\\AppData\\Local\\com.aimeeting.app\\meetings\\meeting-1\\recording.opus',
        format: 'ogg_opus',
        status: 'ready',
        durationMs: 90_000,
        byteSize: 1_048_576,
      },
    }

    render(
      <MeetingWorkspace
        meeting={savedMeeting}
        activeMeetingId={null}
        activeRecordingStatus="idle"
        transcription={{
          status: 'failed', error: null, revision: 0, interimText: '', segments: [],
        }}
        minutes={{
          status: 'pending', error: null, transcriptRevision: 0, content: '',
        }}
        tab="minutes"
        onTabChange={vi.fn()}
        onRetryTranscription={vi.fn()}
        onRetryMinutes={vi.fn()}
      />,
    )

    await user.click(screen.getByRole('button', { name: '播放录音' }))

    expect(screen.getByRole('dialog', { name: '会议录音' })).toBeInTheDocument()
    expect(screen.getByLabelText('会议录音播放器')).toHaveAttribute(
      'src',
      expect.stringContaining('recording.opus'),
    )
    expect(screen.getByText('1:30 · 1.0 MB')).toBeInTheDocument()
  })

  it('does not offer playback while the recording file is still open', () => {
    render(
      <MeetingWorkspace
        meeting={{
          ...meeting,
          audio: {
            relativePath: 'recording.opus',
            playbackPath: 'C:\\recording.opus',
            format: 'ogg_opus',
            status: 'recording',
            durationMs: 0,
            byteSize: 0,
          },
        }}
        activeMeetingId="meeting-1"
        activeRecordingStatus="recording"
        transcription={{
          status: 'streaming', error: null, revision: 0, interimText: '', segments: [],
        }}
        minutes={{
          status: 'pending', error: null, transcriptRevision: 0, content: '',
        }}
        tab="transcript"
        onTabChange={vi.fn()}
        onRetryTranscription={vi.fn()}
        onRetryMinutes={vi.fn()}
      />,
    )

    expect(screen.queryByRole('button', { name: '播放录音' })).not.toBeInTheDocument()
  })
})
