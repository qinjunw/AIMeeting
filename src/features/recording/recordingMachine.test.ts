import { describe, expect, it } from 'vitest'
import {
  createRecordingMachineState,
  recordingMachineReducer,
} from './recordingMachine'

describe('recordingMachineReducer', () => {
  it('covers start, started, pause, resume, stop, and stopped transitions', () => {
    let state = createRecordingMachineState()

    state = recordingMachineReducer(state, {
      type: 'start',
      payload: { meetingId: 'meeting-1' },
    })
    expect(state).toMatchObject({
      meetingId: 'meeting-1',
      recordingStatus: 'preparing',
      transcriptionStatus: 'pending',
      minutesStatus: 'pending',
      runGeneration: 0,
    })

    state = recordingMachineReducer(state, {
      type: 'started',
      payload: { meetingId: 'meeting-1', runGeneration: 1 },
    })
    expect(state.recordingStatus).toBe('recording')

    state = recordingMachineReducer(state, { type: 'pause' })
    expect(state.recordingStatus).toBe('paused')

    state = recordingMachineReducer(state, {
      type: 'resume',
      payload: { runGeneration: 2 },
    })
    expect(state).toMatchObject({
      recordingStatus: 'recording',
      runGeneration: 2,
    })

    state = recordingMachineReducer(state, { type: 'stop' })
    expect(state.recordingStatus).toBe('stopping')

    state = recordingMachineReducer(state, { type: 'stopped' })
    expect(state.recordingStatus).toBe('ready')
  })

  it('recovers an interrupted recording as paused without opening a new run', () => {
    let state = createRecordingMachineState()
    state = recordingMachineReducer(state, {
      type: 'start',
      payload: { meetingId: 'meeting-2' },
    })
    state = recordingMachineReducer(state, {
      type: 'started',
      payload: { meetingId: 'meeting-2', runGeneration: 4 },
    })
    state = recordingMachineReducer(state, { type: 'interrupted' })

    expect(state).toMatchObject({
      recordingStatus: 'interrupted',
      runGeneration: 4,
    })

    state = recordingMachineReducer(state, { type: 'recover' })
    expect(state).toMatchObject({
      recordingStatus: 'paused',
      runGeneration: 4,
    })
  })

  it('keeps recording active when transcription fails', () => {
    const recording = recordingMachineReducer(
      recordingMachineReducer(createRecordingMachineState(), {
        type: 'start',
        payload: { meetingId: 'meeting-3' },
      }),
      {
        type: 'started',
        payload: { meetingId: 'meeting-3', runGeneration: 1 },
      },
    )

    const failed = recordingMachineReducer(recording, {
      type: 'transcription_failed',
      payload: { error: 'provider unavailable' },
    })

    expect(failed.recordingStatus).toBe('recording')
    expect(failed.transcriptionStatus).toBe('failed')
    expect(failed.transcriptionError).toBe('provider unavailable')
  })

  it('updates minutes independently from recording and transcription', () => {
    const state = recordingMachineReducer(createRecordingMachineState(), {
      type: 'minutes_status_changed',
      payload: { status: 'processing' },
    })

    expect(state.recordingStatus).toBe('idle')
    expect(state.transcriptionStatus).toBe('pending')
    expect(state.minutesStatus).toBe('processing')
  })
})
