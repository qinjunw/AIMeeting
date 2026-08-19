export type RecordingStatus =
  | 'preparing'
  | 'recording'
  | 'paused'
  | 'stopping'
  | 'processing'
  | 'ready'
  | 'interrupted'

export type TranscriptionStatus =
  | 'pending'
  | 'streaming'
  | 'processing'
  | 'ready'
  | 'failed'

export type MinutesStatus = 'pending' | 'processing' | 'ready' | 'failed'

export type MeetingStateDto = {
  id: string
  recordingStatus: RecordingStatus
  transcriptionStatus: TranscriptionStatus
  minutesStatus: MinutesStatus
  runGeneration: number
  transcriptRevision: number
  transcriptionError: string | null
}

export type TranscriptSegment = {
  id: string
  runGeneration: number
  revision: number
  text: string
  beginMs: number | null
  endMs: number | null
}
