import { describe, expect, it, vi } from 'vitest'

import { importLegacyMeetingsFromStorage, parseLegacyMeetings } from './legacyImport'

describe('legacy meeting import', () => {
  it('normalizes the old meeting payload into text-only import records', () => {
    const meetings = parseLegacyMeetings(JSON.stringify([{
      id: 'meeting_old_1',
      title: '旧版周会',
      segments: [{ text: '确认周五发布。' }, { text: '负责人是小李。' }],
      digest: { text: '结论：周五发布。' },
      createdAt: '2026-05-01T08:00:00Z',
      updatedAt: '2026-05-01T09:00:00Z',
      stoppedAt: '2026-05-01T09:00:00Z',
    }]))

    expect(meetings).toEqual([expect.objectContaining({
      sourceId: 'meeting_old_1',
      transcript: '确认周五发布。\n负责人是小李。',
      minutes: '结论：周五发布。',
    })])
  })

  it('marks a successful import without deleting the legacy source', async () => {
    const values = new Map<string, string>([[
      'aimeeting.v3.meetings',
      JSON.stringify([{ id: 'meeting_old_1', segments: [], digest: { text: '旧纪要' } }]),
    ]])
    const storage = {
      getItem: vi.fn((key: string) => values.get(key) ?? null),
      setItem: vi.fn((key: string, value: string) => values.set(key, value)),
    }
    const importer = vi.fn().mockResolvedValue(1)

    await expect(importLegacyMeetingsFromStorage(storage, importer)).resolves.toBe(1)
    await expect(importLegacyMeetingsFromStorage(storage, importer)).resolves.toBe(0)

    expect(importer).toHaveBeenCalledTimes(1)
    expect(values.has('aimeeting.v3.meetings')).toBe(true)
    expect(values.get('aimeeting.v4.legacyImportCompleted')).toBe('1')
  })
})
