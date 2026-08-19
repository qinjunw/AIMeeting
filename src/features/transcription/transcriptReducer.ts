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
  if (isStaleIdentity(state, action.payload) || action.payload.revision < state.revision) {
    return state
  }

  if (action.type === 'interim') {
    return {
      ...state,
      revision: action.payload.revision,
      interimText: normalizeTranscriptText(action.payload.text),
    }
  }

  if (state.segments.some((segment) => segment.id === action.payload.segmentId)) {
    return state
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

function isStaleIdentity(
  state: TranscriptState,
  event: TranscriptEventIdentity,
): boolean {
  return (
    event.meetingId !== state.meetingId ||
    event.runGeneration !== state.runGeneration
  )
}
