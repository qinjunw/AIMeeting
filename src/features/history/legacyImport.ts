import {
  importLegacyMeetings,
  type LegacyMeetingImport,
} from '../../bridge/meetingRepositoryClient'

const legacyMeetingsKey = 'aimeeting.v3.meetings'
const importCompletedKey = 'aimeeting.v4.legacyImportCompleted'

type LegacySegment = { text?: unknown }
type LegacyMeeting = {
  id?: unknown
  title?: unknown
  segments?: unknown
  digest?: { text?: unknown }
  createdAt?: unknown
  updatedAt?: unknown
  stoppedAt?: unknown
}

type LegacyStorage = Pick<Storage, 'getItem' | 'setItem'>
type LegacyImporter = (meetings: LegacyMeetingImport[]) => Promise<number>

export async function importLegacyMeetingsFromStorage(
  storage: LegacyStorage = window.localStorage,
  importer: LegacyImporter = importLegacyMeetings,
): Promise<number> {
  if (safeGet(storage, importCompletedKey) === '1') return 0
  const meetings = parseLegacyMeetings(safeGet(storage, legacyMeetingsKey))
  if (meetings.length === 0) {
    safeSet(storage, importCompletedKey, '1')
    return 0
  }
  const imported = await importer(meetings)
  safeSet(storage, importCompletedKey, '1')
  return imported
}

export function parseLegacyMeetings(raw: string | null): LegacyMeetingImport[] {
  if (!raw) return []
  let value: unknown
  try {
    value = JSON.parse(raw)
  } catch {
    return []
  }
  if (!Array.isArray(value)) return []
  return value.flatMap((candidate) => normalizeLegacyMeeting(candidate))
}

function normalizeLegacyMeeting(candidate: unknown): LegacyMeetingImport[] {
  if (!candidate || typeof candidate !== 'object') return []
  const meeting = candidate as LegacyMeeting
  if (typeof meeting.id !== 'string' || !meeting.id.trim()) return []
  const segments = Array.isArray(meeting.segments)
    ? meeting.segments as LegacySegment[]
    : []
  const transcript = segments
    .map((segment) => typeof segment?.text === 'string' ? segment.text.trim() : '')
    .filter(Boolean)
    .join('\n')
  const createdAt = stringValue(meeting.createdAt) ?? new Date(0).toISOString()
  return [{
    sourceId: meeting.id,
    title: stringValue(meeting.title) ?? '旧版会议记录',
    transcript,
    minutes: stringValue(meeting.digest?.text) ?? '',
    createdAt,
    updatedAt: stringValue(meeting.updatedAt) ?? createdAt,
    stoppedAt: stringValue(meeting.stoppedAt),
  }]
}

function stringValue(value: unknown): string | null {
  return typeof value === 'string' && value.trim() ? value.trim() : null
}

function safeGet(storage: LegacyStorage, key: string): string | null {
  try {
    return storage.getItem(key)
  } catch {
    return null
  }
}

function safeSet(storage: LegacyStorage, key: string, value: string) {
  try {
    storage.setItem(key, value)
  } catch {
    // A hardened WebView may disable localStorage; backend data remains unaffected.
  }
}
