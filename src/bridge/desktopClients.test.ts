import { describe, expect, it, vi } from 'vitest'

import { createMeetingRepositoryClient } from './meetingRepositoryClient'
import {
  createProcessingJobClient,
  createProviderClient,
  type SaveProviderProfileRequest,
} from './providerClient'
import type { DesktopTransport } from './transport'

function createTransport() {
  const invoke = vi.fn().mockResolvedValue(undefined)
  const listen = vi.fn().mockResolvedValue(vi.fn())
  const transport: DesktopTransport = { invoke, listen }
  return { transport, invoke, listen }
}

describe('desktop bridge clients', () => {
  it('keeps repository mutations behind typed command wrappers', async () => {
    const { transport, invoke } = createTransport()
    const meeting = {
      id: 'meeting-1',
      title: '设计评审',
      status: 'ready',
      transcriptionStatus: 'ready',
      minutesStatus: 'ready',
      createdAt: '2026-08-19T08:00:00Z',
      updatedAt: '2026-08-19T09:00:00Z',
      deletedAt: null,
    }
    invoke.mockImplementation(async (command: string) => {
      if (command === 'list_meetings') return [meeting]
      if (command === 'get_meeting_detail') {
        return {
          meeting,
          transcriptRevision: 1,
          transcript: '评审通过。',
          minutes: null,
          recording: null,
        }
      }
      return undefined
    })
    const client = createMeetingRepositoryClient(transport)

    await client.list({ deleted: false, limit: 50 })
    await client.get('meeting-1')
    await client.moveToTrash('meeting-1')
    await client.restore('meeting-1')
    await client.permanentlyDelete('meeting-1')

    expect(invoke.mock.calls).toEqual([
      ['list_meetings'],
      ['get_meeting_detail', { request: { meetingId: 'meeting-1' } }],
      ['trash_meetings', { request: { meetingIds: ['meeting-1'] } }],
      ['restore_meetings', { request: { meetingIds: ['meeting-1'] } }],
      [
        'permanently_delete_meetings',
        { request: { meetingIds: ['meeting-1'] } },
      ],
    ])
  })

  it('passes secrets only through save and exposes typed processing jobs', async () => {
    const { transport, invoke, listen } = createTransport()
    const providers = createProviderClient(transport)
    const jobs = createProcessingJobClient(transport)
    const request: SaveProviderProfileRequest = {
      id: 'provider-1',
      capability: 'minutes',
      name: '会议纪要',
      baseUrl: 'https://example.com/v1',
      model: 'model-1',
      endpointFlavor: 'chat-completions',
      apiKey: 'secret',
      isDefault: true,
    }

    await providers.saveProfile(request)
    await jobs.retryTranscription('meeting-1')
    await jobs.retryMinutes('meeting-1', 4)
    await jobs.subscribe(vi.fn())

    expect(invoke.mock.calls).toEqual([
      ['save_provider_profile', { request }],
      ['retry_transcription', { request: { meetingId: 'meeting-1' } }],
      [
        'retry_minutes',
        { request: { meetingId: 'meeting-1', transcriptRevision: 4 } },
      ],
    ])
    expect(listen).toHaveBeenCalledWith('processing-job-event', expect.any(Function))
  })
})
