import { useCallback, useEffect, useRef, useState } from 'react'

import {
  meetingRepositoryClient as defaultMeetingRepositoryClient,
  type ListMeetingsRequest,
  type MeetingDetails,
  type MeetingRepositoryClient,
  type MeetingSummary,
} from '../../bridge/meetingRepositoryClient'

export type MeetingHistoryStatus = 'idle' | 'loading' | 'ready' | 'error'

const defaultRequest: ListMeetingsRequest = {
  deleted: false,
  cursor: null,
  limit: 100,
}

export function useMeetingHistory(
  client: MeetingRepositoryClient = defaultMeetingRepositoryClient,
) {
  const [meetings, setMeetings] = useState<MeetingSummary[]>([])
  const [selectedMeetingId, setSelectedMeetingId] = useState<string | null>(null)
  const [selectedMeeting, setSelectedMeeting] = useState<MeetingDetails | null>(null)
  const [nextCursor, setNextCursor] = useState<string | null>(null)
  const [status, setStatus] = useState<MeetingHistoryStatus>('idle')
  const [error, setError] = useState<string | null>(null)
  const initialized = useRef(false)
  const loadSequence = useRef(0)
  const selectionSequence = useRef(0)
  const requestRef = useRef(defaultRequest)

  const refresh = useCallback(
    async (request: ListMeetingsRequest = requestRef.current) => {
      const sequence = ++loadSequence.current
      requestRef.current = { ...defaultRequest, ...request, cursor: null }
      setStatus('loading')
      setError(null)
      try {
        const page = await client.list(requestRef.current)
        if (sequence !== loadSequence.current) return
        setMeetings(page.items)
        setNextCursor(page.nextCursor)
        setStatus('ready')
      } catch (loadError) {
        if (sequence !== loadSequence.current) return
        setStatus('error')
        setError(errorMessage(loadError))
      }
    },
    [client],
  )

  useEffect(() => {
    if (initialized.current) return
    initialized.current = true
    void refresh()
  }, [refresh])

  const loadMore = useCallback(async () => {
    if (!nextCursor) return
    const sequence = ++loadSequence.current
    setError(null)
    try {
      const page = await client.list({
        ...requestRef.current,
        cursor: nextCursor,
      })
      if (sequence !== loadSequence.current) return
      setMeetings((current) => [...current, ...page.items])
      setNextCursor(page.nextCursor)
      setStatus('ready')
    } catch (loadError) {
      if (sequence !== loadSequence.current) return
      setStatus('error')
      setError(errorMessage(loadError))
    }
  }, [client, nextCursor])

  const selectMeeting = useCallback(
    async (meetingId: string | null) => {
      const sequence = ++selectionSequence.current
      setSelectedMeetingId(meetingId)
      setSelectedMeeting(null)
      if (!meetingId) return null
      try {
        const details = await client.get(meetingId)
        if (sequence === selectionSequence.current) setSelectedMeeting(details)
        return details
      } catch (selectionError) {
        if (sequence === selectionSequence.current) {
          setError(errorMessage(selectionError))
        }
        throw selectionError
      }
    },
    [client],
  )

  const renameMeeting = useCallback(
    async (meetingId: string, title: string) => {
      const updated = await client.rename(meetingId, title)
      setMeetings((current) =>
        current.map((meeting) => (meeting.id === meetingId ? updated : meeting)),
      )
      setSelectedMeeting((current) =>
        current?.id === meetingId ? { ...current, title: updated.title } : current,
      )
      return updated
    },
    [client],
  )

  const removeLocally = useCallback((meetingId: string) => {
    setMeetings((current) => current.filter((meeting) => meeting.id !== meetingId))
    setSelectedMeetingId((current) => (current === meetingId ? null : current))
    setSelectedMeeting((current) => (current?.id === meetingId ? null : current))
  }, [])

  const moveToTrash = useCallback(
    async (meetingId: string) => {
      await client.moveToTrash(meetingId)
      removeLocally(meetingId)
    },
    [client, removeLocally],
  )

  const restore = useCallback(
    async (meetingId: string) => {
      await client.restore(meetingId)
      removeLocally(meetingId)
    },
    [client, removeLocally],
  )

  const permanentlyDelete = useCallback(
    async (meetingId: string) => {
      await client.permanentlyDelete(meetingId)
      removeLocally(meetingId)
    },
    [client, removeLocally],
  )

  const emptyTrash = useCallback(async () => {
    await client.emptyTrash()
    setMeetings([])
    setSelectedMeetingId(null)
    setSelectedMeeting(null)
    setNextCursor(null)
  }, [client])

  return {
    meetings,
    selectedMeetingId,
    selectedMeeting,
    nextCursor,
    status,
    error,
    refresh,
    loadMore,
    selectMeeting,
    renameMeeting,
    moveToTrash,
    restore,
    permanentlyDelete,
    emptyTrash,
  }
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}
