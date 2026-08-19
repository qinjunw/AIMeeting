import { useCallback, useEffect, useRef, useState } from 'react'

import {
  recordingClient as defaultRecordingClient,
  type AudioSourceSelection,
  type MeetingSessionEvent,
  type RecordingClient,
  type SessionSnapshot,
  type StartRecordingRequest,
} from '../../bridge/recordingClient'
import {
  processingJobClient as defaultProcessingJobClient,
  type ProcessingJob,
  type ProcessingJobClient,
  type ProcessingJobEvent,
} from '../../bridge/providerClient'
import type {
  MinutesStatus,
  RecordingStatus,
  TranscriptSegment,
  TranscriptionStatus,
} from '../../domain/meeting'
import {
  transcriptReducer,
  type TranscriptState,
} from '../transcription/transcriptReducer'

export type RecordingViewState = {
  meetingId: string | null
  runGeneration: number
  status: RecordingStatus | 'idle'
  error: string | null
}

export type TranscriptionViewState = {
  status: TranscriptionStatus
  error: string | null
  revision: number
  interimText: string
  segments: TranscriptSegment[]
}

export type MinutesViewState = {
  status: MinutesStatus
  error: string | null
  transcriptRevision: number
  content: string
}

export type UseMeetingSessionOptions = {
  recordingClient?: RecordingClient
  processingJobClient?: ProcessingJobClient
}

type ActiveIdentity = {
  meetingId: string
  runGeneration: number
  transcriptRevision: number
  minutesRevision: number
}

const initialRecording: RecordingViewState = {
  meetingId: null,
  runGeneration: 0,
  status: 'idle',
  error: null,
}

const initialTranscription: TranscriptionViewState = {
  status: 'pending',
  error: null,
  revision: 0,
  interimText: '',
  segments: [],
}

const initialMinutes: MinutesViewState = {
  status: 'pending',
  error: null,
  transcriptRevision: 0,
  content: '',
}

export function useMeetingSession(options: UseMeetingSessionOptions = {}) {
  const recordingClient = options.recordingClient ?? defaultRecordingClient
  const processingJobClient =
    options.processingJobClient ?? defaultProcessingJobClient
  const [recording, setRecording] = useState(initialRecording)
  const [transcription, setTranscription] = useState(initialTranscription)
  const [minutes, setMinutes] = useState(initialMinutes)
  const activeIdentity = useRef<ActiveIdentity | null>(null)
  const commandSequence = useRef(0)
  const selectedSources = useRef<AudioSourceSelection>({
    microphone: true,
    systemAudio: true,
  })

  const applySnapshot = useCallback(
    (snapshot: SessionSnapshot, resetContent: boolean) => {
      activeIdentity.current = {
        meetingId: snapshot.meetingId,
        runGeneration: snapshot.runGeneration,
        transcriptRevision: snapshot.transcriptRevision,
        minutesRevision: resetContent ? 0 : activeIdentity.current?.minutesRevision ?? 0,
      }
      setRecording({
        meetingId: snapshot.meetingId,
        runGeneration: snapshot.runGeneration,
        status: snapshot.recordingStatus,
        error: null,
      })
      setTranscription((current) => ({
        status: snapshot.transcriptionStatus,
        error: snapshot.transcriptionError,
        revision: snapshot.transcriptRevision,
        interimText: resetContent ? '' : current.interimText,
        segments: resetContent ? [] : current.segments,
      }))
      setMinutes((current) => ({
        status: snapshot.minutesStatus,
        error: snapshot.minutesError,
        transcriptRevision: resetContent ? 0 : current.transcriptRevision,
        content: resetContent ? '' : current.content,
      }))
    },
    [],
  )

  const handleEvent = useCallback((event: MeetingSessionEvent) => {
    const identity = activeIdentity.current
    if (
      !identity ||
      event.meetingId !== identity.meetingId ||
      event.runGeneration !== identity.runGeneration
    ) {
      return
    }

    if (event.kind === 'meeting-state') {
      setRecording((current) => ({
        ...current,
        status: event.recordingStatus,
        error: null,
      }))
      if (
        event.transcriptRevision !== undefined &&
        event.transcriptRevision >= identity.transcriptRevision
      ) {
        identity.transcriptRevision = event.transcriptRevision
        setTranscription((current) => ({
          ...current,
          revision: event.transcriptRevision ?? current.revision,
          status: event.transcriptionStatus ?? current.status,
          error:
            event.transcriptionError === undefined
              ? current.error
              : event.transcriptionError,
        }))
      }
      if (event.minutesStatus !== undefined) {
        setMinutes((current) => ({
          ...current,
          status: event.minutesStatus ?? current.status,
          error: event.minutesError === undefined ? current.error : event.minutesError,
        }))
      }
      return
    }

    if (event.kind === 'minutes-status') {
      if (
        event.transcriptRevision < identity.transcriptRevision ||
        event.transcriptRevision < identity.minutesRevision
      ) {
        return
      }
      identity.minutesRevision = event.transcriptRevision
      setMinutes({
        status: event.status,
        error: event.error,
        transcriptRevision: event.transcriptRevision,
        content: event.content ?? '',
      })
      return
    }

    if (event.revision < identity.transcriptRevision) return
    identity.transcriptRevision = event.revision

    if (event.kind === 'transcription-status') {
      setTranscription((current) => ({
        ...current,
        status: event.status,
        error: event.error,
        revision: event.revision,
      }))
      return
    }

    setTranscription((current) => {
      const transcriptState: TranscriptState = {
        meetingId: identity.meetingId,
        runGeneration: identity.runGeneration,
        revision: current.revision,
        interimText: current.interimText,
        segments: current.segments,
      }
      const next = transcriptReducer(
        transcriptState,
        event.kind === 'transcript-interim'
          ? { type: 'interim', payload: event }
          : { type: 'final', payload: event },
      )
      return {
        status: 'streaming',
        error: null,
        revision: next.revision,
        interimText: next.interimText,
        segments: next.segments,
      }
    })
  }, [])

  const handleProcessingJobEvent = useCallback((event: ProcessingJobEvent) => {
    const identity = activeIdentity.current
    if (
      !identity ||
      event.meetingId !== identity.meetingId ||
      event.runGeneration !== identity.runGeneration ||
      event.revision < identity.transcriptRevision
    ) {
      return
    }

    if (event.status === 'superseded') return

    if (event.kind === 'file_transcription') {
      identity.transcriptRevision = event.revision
      setTranscription((current) => ({
        ...current,
        status: transcriptionStatusForJob(event.status),
        error: event.status === 'failed' ? event.errorSummary : null,
        revision: event.revision,
      }))
      return
    }

    if (event.revision < identity.minutesRevision) return
    identity.minutesRevision = event.revision
    setMinutes((current) => ({
      ...current,
      status: minutesStatusForJob(event.status),
      error: event.status === 'failed' ? event.errorSummary : null,
      transcriptRevision: event.revision,
    }))
  }, [])

  useEffect(() => {
    let disposed = false
    let unsubscribe: (() => void) | undefined

    void recordingClient
      .subscribe(handleEvent)
      .then((nextUnsubscribe) => {
        if (disposed) {
          nextUnsubscribe()
        } else {
          unsubscribe = nextUnsubscribe
        }
      })
      .catch((error: unknown) => {
        if (!disposed) {
          setRecording((current) => ({
            ...current,
            error: errorMessage(error),
          }))
        }
      })

    return () => {
      disposed = true
      unsubscribe?.()
    }
  }, [handleEvent, recordingClient])

  useEffect(() => {
    let disposed = false
    let unsubscribe: (() => void) | undefined

    void processingJobClient
      .subscribe(handleProcessingJobEvent)
      .then((nextUnsubscribe) => {
        if (disposed) {
          nextUnsubscribe()
        } else {
          unsubscribe = nextUnsubscribe
        }
      })
      .catch((error: unknown) => {
        if (!disposed) {
          setTranscription((current) => ({
            ...current,
            error: errorMessage(error),
          }))
        }
      })

    return () => {
      disposed = true
      unsubscribe?.()
    }
  }, [handleProcessingJobEvent, processingJobClient])

  const start = useCallback(
    async (request: StartRecordingRequest) => {
      const sequence = ++commandSequence.current
      selectedSources.current = request.sources
      try {
        const snapshot = await recordingClient.start(request)
        if (sequence === commandSequence.current) applySnapshot(snapshot, true)
        return snapshot
      } catch (error) {
        if (sequence === commandSequence.current) {
          setRecording((current) => ({ ...current, error: errorMessage(error) }))
        }
        throw error
      }
    },
    [applySnapshot, recordingClient],
  )

  const runLifecycleCommand = useCallback(
    async (
      operation: 'pause' | 'resume' | 'stop',
    ): Promise<SessionSnapshot | null> => {
      const identity = activeIdentity.current
      if (!identity) return null
      const sequence = ++commandSequence.current
      try {
        const identityRequest = {
          meetingId: identity.meetingId,
          runGeneration: identity.runGeneration,
        }
        const snapshot =
          operation === 'resume'
            ? await recordingClient.resume({
                ...identityRequest,
                sources: selectedSources.current,
              })
            : await recordingClient[operation](identityRequest)
        if (sequence === commandSequence.current) applySnapshot(snapshot, false)
        return snapshot
      } catch (error) {
        if (sequence === commandSequence.current) {
          setRecording((current) => ({ ...current, error: errorMessage(error) }))
        }
        throw error
      }
    },
    [applySnapshot, recordingClient],
  )

  const retryTranscription = useCallback(async (): Promise<ProcessingJob | null> => {
    const identity = activeIdentity.current
    if (!identity) return null
    setTranscription((current) => ({
      ...current,
      status: 'processing',
      error: null,
    }))
    try {
      return await processingJobClient.retryTranscription(identity.meetingId)
    } catch (error) {
      setTranscription((current) => ({
        ...current,
        status: 'failed',
        error: errorMessage(error),
      }))
      throw error
    }
  }, [processingJobClient])

  const retryMinutes = useCallback(async (): Promise<ProcessingJob | null> => {
    const identity = activeIdentity.current
    if (!identity) return null
    setMinutes((current) => ({ ...current, status: 'processing', error: null }))
    try {
      return await processingJobClient.retryMinutes(
        identity.meetingId,
        identity.transcriptRevision,
      )
    } catch (error) {
      setMinutes((current) => ({
        ...current,
        status: 'failed',
        error: errorMessage(error),
      }))
      throw error
    }
  }, [processingJobClient])

  return {
    recording,
    transcription,
    minutes,
    start,
    pause: () => runLifecycleCommand('pause'),
    resume: () => runLifecycleCommand('resume'),
    stop: () => runLifecycleCommand('stop'),
    retryTranscription,
    retryMinutes,
  }
}

export type { AudioSourceSelection }

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

function transcriptionStatusForJob(
  status: ProcessingJob['status'],
): TranscriptionStatus {
  if (status === 'failed') return 'failed'
  if (status === 'succeeded') return 'ready'
  return 'processing'
}

function minutesStatusForJob(status: ProcessingJob['status']): MinutesStatus {
  if (status === 'failed') return 'failed'
  if (status === 'succeeded') return 'ready'
  return 'processing'
}
