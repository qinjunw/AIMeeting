import type {
  MinutesStatus,
  RecordingStatus,
  TranscriptionStatus,
} from '../../domain/meeting'

export type RecordingMachineState = {
  meetingId: string | null
  recordingStatus: RecordingStatus | 'idle'
  transcriptionStatus: TranscriptionStatus
  minutesStatus: MinutesStatus
  runGeneration: number
  transcriptionError: string | null
}

export type RecordingMachineAction =
  | { type: 'start'; payload: { meetingId: string } }
  | {
      type: 'started'
      payload: { meetingId: string; runGeneration: number }
    }
  | { type: 'pause' }
  | { type: 'resume'; payload: { runGeneration: number } }
  | { type: 'stop' }
  | { type: 'stopped' }
  | { type: 'interrupted' }
  | { type: 'recover' }
  | {
      type: 'transcription_status_changed'
      payload: { status: Exclude<TranscriptionStatus, 'failed'> }
    }
  | { type: 'transcription_failed'; payload: { error: string } }
  | { type: 'minutes_status_changed'; payload: { status: MinutesStatus } }

export function createRecordingMachineState(): RecordingMachineState {
  return {
    meetingId: null,
    recordingStatus: 'idle',
    transcriptionStatus: 'pending',
    minutesStatus: 'pending',
    runGeneration: 0,
    transcriptionError: null,
  }
}

export function recordingMachineReducer(
  state: RecordingMachineState,
  action: RecordingMachineAction,
): RecordingMachineState {
  switch (action.type) {
    case 'start':
      if (state.recordingStatus !== 'idle' && state.recordingStatus !== 'ready') {
        return state
      }
      return {
        meetingId: action.payload.meetingId,
        recordingStatus: 'preparing',
        transcriptionStatus: 'pending',
        minutesStatus: 'pending',
        runGeneration: 0,
        transcriptionError: null,
      }
    case 'started':
      if (
        state.recordingStatus !== 'preparing' ||
        state.meetingId !== action.payload.meetingId ||
        action.payload.runGeneration <= state.runGeneration
      ) {
        return state
      }
      return {
        ...state,
        recordingStatus: 'recording',
        runGeneration: action.payload.runGeneration,
      }
    case 'pause':
      return state.recordingStatus === 'recording'
        ? { ...state, recordingStatus: 'paused' }
        : state
    case 'resume':
      if (
        state.recordingStatus !== 'paused' ||
        action.payload.runGeneration <= state.runGeneration
      ) {
        return state
      }
      return {
        ...state,
        recordingStatus: 'recording',
        runGeneration: action.payload.runGeneration,
      }
    case 'stop':
      return state.recordingStatus === 'recording' || state.recordingStatus === 'paused'
        ? { ...state, recordingStatus: 'stopping' }
        : state
    case 'stopped':
      return state.recordingStatus === 'stopping'
        ? { ...state, recordingStatus: 'ready' }
        : state
    case 'interrupted':
      return isInterruptible(state.recordingStatus)
        ? { ...state, recordingStatus: 'interrupted' }
        : state
    case 'recover':
      return state.recordingStatus === 'interrupted'
        ? { ...state, recordingStatus: 'paused' }
        : state
    case 'transcription_status_changed':
      return {
        ...state,
        transcriptionStatus: action.payload.status,
        transcriptionError: null,
      }
    case 'transcription_failed':
      return {
        ...state,
        transcriptionStatus: 'failed',
        transcriptionError: action.payload.error,
      }
    case 'minutes_status_changed':
      return { ...state, minutesStatus: action.payload.status }
  }
}

function isInterruptible(status: RecordingMachineState['recordingStatus']): boolean {
  return (
    status === 'preparing' ||
    status === 'recording' ||
    status === 'paused' ||
    status === 'stopping' ||
    status === 'processing'
  )
}
