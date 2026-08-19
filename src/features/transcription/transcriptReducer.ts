import type { TranscriptSegment } from '../../domain/meeting'
import { normalizeTranscriptText } from '../../services/chineseText'

export type TranscriptState = {
  meetingId: string
  runGeneration: number
  revision: number
  interimText: string
  segments: TranscriptSegment[]
}

type TranscriptEventIdentity = {
  meetingId: string
  runGeneration: number
  revision: number
}

export type TranscriptReducerAction =
  | {
      type: 'interim'
      payload: TranscriptEventIdentity & { text: string }
    }
  | {
      type: 'final'
      payload: TranscriptEventIdentity & {
        segmentId: string
        text: string
        beginMs: number | null
        endMs: number | null
      }
    }

export function createTranscriptState(
  meetingId: string,
  runGeneration: number,
): TranscriptState {
  return {
    meetingId,
    runGeneration,
    revision: 0,
    interimText: '',
    segments: [],
  }
}

export function transcriptReducer(
  state: TranscriptState,
  action: TranscriptReducerAction,
): TranscriptState {
  if (isStaleEvent(state, action.payload)) {
    return state
  }

  if (action.type === 'interim') {
    return {
      ...state,
      revision: action.payload.revision,
      interimText: action.payload.text,
    }
  }

  const text = normalizeTranscriptText(action.payload.text)
  return {
    ...state,
    revision: action.payload.revision,
    interimText: '',
    segments: text
      ? [
          ...state.segments,
          {
            id: action.payload.segmentId,
            runGeneration: action.payload.runGeneration,
            revision: action.payload.revision,
            text,
            beginMs: action.payload.beginMs,
            endMs: action.payload.endMs,
          },
        ]
      : state.segments,
  }
}

function isStaleEvent(
  state: TranscriptState,
  event: TranscriptEventIdentity,
): boolean {
  return (
    event.meetingId !== state.meetingId ||
    event.runGeneration !== state.runGeneration ||
    event.revision <= state.revision
  )
}
