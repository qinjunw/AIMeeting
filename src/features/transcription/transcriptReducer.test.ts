import { describe, expect, it } from 'vitest'
import { createTranscriptState, transcriptReducer } from './transcriptReducer'

describe('transcriptReducer', () => {
  it('replaces interim text instead of appending it', () => {
    let state = createTranscriptState('meeting-1', 3)

    state = transcriptReducer(state, {
      type: 'interim',
      payload: {
        meetingId: 'meeting-1',
        runGeneration: 3,
        revision: 1,
        text: '第一版',
      },
    })
    state = transcriptReducer(state, {
      type: 'interim',
      payload: {
        meetingId: 'meeting-1',
        runGeneration: 3,
        revision: 2,
        text: '第二版实时文字',
      },
    })

    expect(state.interimText).toBe('第二版实时文字')
    expect(state.segments).toEqual([])
    expect(state.revision).toBe(2)
  })

  it('normalizes and appends final text, then clears interim text', () => {
    let state = createTranscriptState('meeting-1', 3)
    state = transcriptReducer(state, {
      type: 'interim',
      payload: {
        meetingId: 'meeting-1',
        runGeneration: 3,
        revision: 1,
        text: '我們正在開會',
      },
    })

    state = transcriptReducer(state, {
      type: 'final',
      payload: {
        meetingId: 'meeting-1',
        runGeneration: 3,
        revision: 2,
        segmentId: 'segment-1',
        text: '  我們\n正在   開會  ',
        beginMs: 100,
        endMs: 900,
      },
    })

    expect(state.interimText).toBe('')
    expect(state.segments).toEqual([
      {
        id: 'segment-1',
        runGeneration: 3,
        revision: 2,
        text: '我们 正在 开会',
        beginMs: 100,
        endMs: 900,
      },
    ])
  })

  it.each([
    {
      name: 'another meeting',
      meetingId: 'meeting-old',
      runGeneration: 5,
      revision: 5,
    },
    {
      name: 'an old run generation',
      meetingId: 'meeting-1',
      runGeneration: 4,
      revision: 5,
    },
    {
      name: 'an old revision',
      meetingId: 'meeting-1',
      runGeneration: 5,
      revision: 3,
    },
  ])('ignores final events from $name', ({ meetingId, runGeneration, revision }) => {
    const current = {
      ...createTranscriptState('meeting-1', 5),
      revision: 4,
      interimText: '当前内容',
    }

    const next = transcriptReducer(current, {
      type: 'final',
      payload: {
        meetingId,
        runGeneration,
        revision,
        segmentId: 'stale-segment',
        text: '不应该进入当前会议',
        beginMs: 0,
        endMs: 10,
      },
    })

    expect(next).toBe(current)
  })

  it('ignores a duplicate final revision', () => {
    const state = transcriptReducer(createTranscriptState('meeting-1', 1), {
      type: 'final',
      payload: {
        meetingId: 'meeting-1',
        runGeneration: 1,
        revision: 1,
        segmentId: 'segment-1',
        text: '第一句',
        beginMs: 0,
        endMs: 100,
      },
    })

    const duplicate = transcriptReducer(state, {
      type: 'final',
      payload: {
        meetingId: 'meeting-1',
        runGeneration: 1,
        revision: 1,
        segmentId: 'segment-1',
        text: '第一句',
        beginMs: 0,
        endMs: 100,
      },
    })

    expect(duplicate).toBe(state)
    expect(duplicate.segments).toHaveLength(1)
  })
})
