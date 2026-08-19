import type {
  MinutesStatus,
  RecordingStatus,
  TranscriptionStatus,
} from '../domain/meeting'
import {
  tauriTransport,
  type DesktopTransport,
  type Unsubscribe,
} from './transport'

export type AudioSourceSelection = {
  microphone: boolean
  systemAudio: boolean
}

export type StartRecordingRequest = {
  title: string
  sources: AudioSourceSelection
}

export type RecordingIdentityRequest = {
  meetingId: string
  runGeneration: number
}

export type ResumeRecordingRequest = RecordingIdentityRequest & {
  sources: AudioSourceSelection
}

export type SessionSnapshot = {
  meetingId: string
  runGeneration: number
  recordingStatus: RecordingStatus
  transcriptRevision: number
  transcriptionStatus: TranscriptionStatus
  transcriptionError: string | null
  minutesStatus: MinutesStatus
  minutesError: string | null
}

export type MeetingStateEvent = {
  kind: 'meeting-state'
  meetingId: string
  runGeneration: number
  recordingStatus: RecordingStatus
  transcriptionStatus?: TranscriptionStatus
  transcriptionError?: string | null
  minutesStatus?: MinutesStatus
  minutesError?: string | null
  transcriptRevision?: number
}

export type TranscriptionStatusEvent = {
  kind: 'transcription-status'
  meetingId: string
  runGeneration: number
  revision: number
  status: TranscriptionStatus
  error: string | null
}

export type TranscriptInterimEvent = {
  kind: 'transcript-interim'
  meetingId: string
  runGeneration: number
  revision: number
  text: string
}

export type TranscriptFinalEvent = {
  kind: 'transcript-final'
  meetingId: string
  runGeneration: number
  revision: number
  segmentId: string
  text: string
  beginMs: number | null
  endMs: number | null
}

export type MinutesStatusEvent = {
  kind: 'minutes-status'
  meetingId: string
  runGeneration: number
  transcriptRevision: number
  status: MinutesStatus
  content: string | null
  error: string | null
}

export type MeetingSessionEvent =
  | MeetingStateEvent
  | TranscriptionStatusEvent
  | TranscriptInterimEvent
  | TranscriptFinalEvent
  | MinutesStatusEvent

type MeetingStateEventPayload = Omit<MeetingStateEvent, 'kind'>

type TranscriptionEventPayload =
  | (Omit<TranscriptionStatusEvent, 'kind'> & { event: 'status' })
  | (Omit<TranscriptInterimEvent, 'kind'> & { event: 'interim' })
  | (Omit<TranscriptFinalEvent, 'kind'> & { event: 'final' })

type MinutesEventPayload = Omit<MinutesStatusEvent, 'kind'>

export type RecordingClient = {
  start(request: StartRecordingRequest): Promise<SessionSnapshot>
  pause(request: RecordingIdentityRequest): Promise<SessionSnapshot>
  resume(request: ResumeRecordingRequest): Promise<SessionSnapshot>
  stop(request: RecordingIdentityRequest): Promise<SessionSnapshot>
  getActive(): Promise<SessionSnapshot | null>
  subscribe(listener: (event: MeetingSessionEvent) => void): Promise<Unsubscribe>
}

export function createRecordingClient(
  transport: DesktopTransport = tauriTransport,
): RecordingClient {
  return {
    start: (request) => invokeRequest(transport, 'start_recording', request),
    pause: (request) => invokeRequest(transport, 'pause_recording', request),
    resume: (request) => invokeRequest(transport, 'resume_recording', request),
    stop: (request) => invokeRequest(transport, 'stop_recording', request),
    getActive: () => transport.invoke<SessionSnapshot | null>('get_active_meeting'),
    async subscribe(listener) {
      const unlisteners: Unsubscribe[] = []
      try {
        unlisteners.push(
          await transport.listen<MeetingStateEventPayload>(
            'meeting-state-event',
            (payload) => listener({ kind: 'meeting-state', ...payload }),
          ),
        )
        unlisteners.push(
          await transport.listen<TranscriptionEventPayload>(
            'transcription-event',
            (payload) => listener(mapTranscriptionEvent(payload)),
          ),
        )
        unlisteners.push(
          await transport.listen<MinutesEventPayload>('minutes-event', (payload) =>
            listener({ kind: 'minutes-status', ...payload }),
          ),
        )
      } catch (error) {
        unlisteners.reverse().forEach((unlisten) => unlisten())
        throw error
      }

      let active = true
      return () => {
        if (!active) return
        active = false
        unlisteners.reverse().forEach((unlisten) => unlisten())
      }
    },
  }
}

export const recordingClient = createRecordingClient()

function invokeRequest<TRequest extends object>(
  transport: DesktopTransport,
  command: string,
  request: TRequest,
): Promise<SessionSnapshot> {
  return transport.invoke<SessionSnapshot>(command, { request })
}

function mapTranscriptionEvent(
  payload: TranscriptionEventPayload,
): TranscriptionStatusEvent | TranscriptInterimEvent | TranscriptFinalEvent {
  if (payload.event === 'status') {
    const { event: _event, ...status } = payload
    return { kind: 'transcription-status', ...status }
  }
  if (payload.event === 'interim') {
    const { event: _event, ...interim } = payload
    return { kind: 'transcript-interim', ...interim }
  }
  const { event: _event, ...final } = payload
  return { kind: 'transcript-final', ...final }
}
