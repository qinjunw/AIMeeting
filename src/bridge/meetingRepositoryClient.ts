import type {
  MinutesStatus,
  RecordingStatus,
  TranscriptionStatus,
} from '../domain/meeting'
import { tauriTransport, type DesktopTransport } from './transport'

export type MeetingSummary = {
  id: string
  title: string
  recordingStatus: RecordingStatus
  transcriptionStatus: TranscriptionStatus
  minutesStatus: MinutesStatus
  createdAt: string
  updatedAt: string
  durationMs: number
  deletedAt: string | null
}

export type MeetingMinutes = {
  transcriptRevision: number
  content: string
  providerLabel: string
}

export type MeetingAudioAsset = {
  relativePath: string
  playbackPath: string | null
  format: string
  status: string
  durationMs: number
  byteSize: number
}

export type MeetingDetails = MeetingSummary & {
  transcriptRevision: number
  transcript: string
  minutes: MeetingMinutes | null
  audio: MeetingAudioAsset | null
}

export type MeetingPage = {
  items: MeetingSummary[]
  nextCursor: string | null
}

export type LegacyMeetingImport = {
  sourceId: string
  title: string
  transcript: string
  minutes: string
  createdAt: string
  updatedAt: string
  stoppedAt: string | null
}

export type ListMeetingsRequest = {
  deleted?: boolean
  cursor?: string | null
  limit?: number
}

export type MeetingRepositoryClient = {
  list(request?: ListMeetingsRequest): Promise<MeetingPage>
  get(meetingId: string): Promise<MeetingDetails>
  rename(meetingId: string, title: string): Promise<MeetingSummary>
  moveToTrash(meetingId: string): Promise<void>
  restore(meetingId: string): Promise<void>
  permanentlyDelete(meetingId: string): Promise<void>
  emptyTrash(): Promise<void>
}

export function createMeetingRepositoryClient(
  transport: DesktopTransport = tauriTransport,
): MeetingRepositoryClient {
  return {
    async list(request = {}) {
      const command = request.deleted ? 'list_trash' : 'list_meetings'
      const rows = await transport.invoke<WireMeetingRecord[]>(command)
      return { items: rows.map(mapMeetingSummary), nextCursor: null }
    },
    async get(meetingId) {
      const detail = await transport.invoke<WireMeetingDetail | null>(
        'get_meeting_detail',
        { request: { meetingId } },
      )
      if (!detail) throw new Error('会议记录不存在。')
      return mapMeetingDetails(detail)
    },
    async rename(meetingId, title) {
      await transport.invoke<void>('rename_meeting', {
        request: { meetingId, title },
      })
      const detail = await transport.invoke<WireMeetingDetail | null>(
        'get_meeting_detail',
        { request: { meetingId } },
      )
      if (!detail) throw new Error('会议记录不存在。')
      return mapMeetingSummary(detail.meeting)
    },
    moveToTrash: (meetingId) =>
      transport.invoke<void>('trash_meetings', {
        request: { meetingIds: [meetingId] },
      }),
    restore: (meetingId) =>
      transport.invoke<void>('restore_meetings', {
        request: { meetingIds: [meetingId] },
      }),
    permanentlyDelete: (meetingId) =>
      transport.invoke<void>('permanently_delete_meetings', {
        request: { meetingIds: [meetingId] },
      }),
    async emptyTrash() {
      const rows = await transport.invoke<WireMeetingRecord[]>('list_trash')
      if (rows.length === 0) return
      await transport.invoke<void>('permanently_delete_meetings', {
        request: { meetingIds: rows.map((row) => row.id) },
      })
    },
  }
}

export const meetingRepositoryClient = createMeetingRepositoryClient()

export async function importLegacyMeetings(
  meetings: LegacyMeetingImport[],
  transport: DesktopTransport = tauriTransport,
): Promise<number> {
  const result = await transport.invoke<{ imported: number }>('import_legacy_meetings', {
    request: { meetings },
  })
  return result.imported
}

type WireMeetingRecord = {
  id: string
  title: string
  status: RecordingStatus
  transcriptionStatus: TranscriptionStatus
  minutesStatus: MinutesStatus
  createdAt: string
  updatedAt: string
  deletedAt: string | null
}

type WireMeetingMinutes = {
  revision: number
  content: string
  providerLabel: string
}

type WireRecordingAsset = Omit<MeetingAudioAsset, 'playbackPath'>

type WireMeetingDetail = {
  meeting: WireMeetingRecord
  transcriptRevision: number
  transcript: string
  minutes: WireMeetingMinutes | null
  recording: WireRecordingAsset | null
  recordingPlaybackPath: string | null
}

function mapMeetingSummary(row: WireMeetingRecord): MeetingSummary {
  return {
    id: row.id,
    title: row.title,
    recordingStatus: row.status,
    transcriptionStatus: row.transcriptionStatus,
    minutesStatus: row.minutesStatus,
    createdAt: row.createdAt,
    updatedAt: row.updatedAt,
    durationMs: 0,
    deletedAt: row.deletedAt,
  }
}

function mapMeetingDetails(detail: WireMeetingDetail): MeetingDetails {
  const summary = mapMeetingSummary(detail.meeting)
  return {
    ...summary,
    durationMs: detail.recording?.durationMs ?? 0,
    transcriptRevision: detail.transcriptRevision,
    transcript: detail.transcript,
    minutes: detail.minutes
      ? {
          transcriptRevision: detail.minutes.revision,
          content: detail.minutes.content,
          providerLabel: detail.minutes.providerLabel,
        }
      : null,
    audio: detail.recording
      ? { ...detail.recording, playbackPath: detail.recordingPlaybackPath }
      : null,
  }
}
