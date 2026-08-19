import { useEffect, useRef, useState } from 'react'

import type { AudioSourceSelection } from './bridge/recordingClient'
import { HistorySidebar } from './features/history/HistorySidebar'
import { importLegacyMeetingsFromStorage } from './features/history/legacyImport'
import { useMeetingHistory } from './features/history/useMeetingHistory'
import { MeetingWorkspace, type WorkspaceTab } from './features/meeting/MeetingWorkspace'
import { RecordingBar } from './features/meeting/RecordingBar'
import { useMeetingSession } from './features/meeting/useMeetingSession'
import { SettingsDialog } from './features/settings/SettingsDialog'
import { useProviderSettings } from './features/settings/useProviderSettings'

export default function App() {
  const session = useMeetingSession()
  const history = useMeetingHistory()
  const providerSettings = useProviderSettings()
  const [sources, setSources] = useState<AudioSourceSelection>({
    microphone: true,
    systemAudio: true,
  })
  const [tab, setTab] = useState<WorkspaceTab>('minutes')
  const [trashMode, setTrashMode] = useState(false)
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [commandBusy, setCommandBusy] = useState(false)
  const [elapsedMs, setElapsedMs] = useState(0)
  const elapsedBeforeRun = useRef(0)
  const runStartedAt = useRef<number | null>(null)
  const legacyImportStarted = useRef(false)

  useEffect(() => {
    if (legacyImportStarted.current) return
    legacyImportStarted.current = true
    void importLegacyMeetingsFromStorage()
      .then((imported) => imported > 0 ? history.refresh({ deleted: false }) : undefined)
      .catch(() => undefined)
  }, [history.refresh])

  useEffect(() => {
    const status = session.recording.status
    if (status === 'recording' && runStartedAt.current === null) {
      runStartedAt.current = Date.now()
    }
    if (status !== 'recording' && runStartedAt.current !== null) {
      elapsedBeforeRun.current += Date.now() - runStartedAt.current
      runStartedAt.current = null
      setElapsedMs(elapsedBeforeRun.current)
    }
    if (status !== 'recording') return
    const timer = window.setInterval(() => {
      setElapsedMs(
        elapsedBeforeRun.current + Date.now() - (runStartedAt.current ?? Date.now()),
      )
    }, 250)
    return () => window.clearInterval(timer)
  }, [session.recording.status])

  useEffect(() => {
    if (
      history.status === 'ready' &&
      history.meetings.length > 0 &&
      !history.selectedMeetingId &&
      !session.recording.meetingId
    ) {
      void history.selectMeeting(history.meetings[0].id)
    }
  }, [
    history.status,
    history.meetings,
    history.selectedMeetingId,
    history.selectMeeting,
    session.recording.meetingId,
  ])

  const start = async () => {
    setCommandBusy(true)
    elapsedBeforeRun.current = 0
    runStartedAt.current = Date.now()
    setElapsedMs(0)
    try {
      const snapshot = await session.start({ title: defaultMeetingTitle(), sources })
      await history.refresh({ deleted: false })
      await history.selectMeeting(snapshot.meetingId)
      setTrashMode(false)
      setTab('transcript')
    } finally {
      setCommandBusy(false)
    }
  }

  const pause = async () => {
    setCommandBusy(true)
    try {
      await session.pause()
      await history.refresh({ deleted: false })
    } finally {
      setCommandBusy(false)
    }
  }

  const resume = async () => {
    setCommandBusy(true)
    try {
      await session.resume()
      await history.refresh({ deleted: false })
    } finally {
      setCommandBusy(false)
    }
  }

  const stop = async () => {
    setCommandBusy(true)
    try {
      const snapshot = await session.stop()
      await history.refresh({ deleted: false })
      if (snapshot) await history.selectMeeting(snapshot.meetingId)
      setTab('minutes')
    } finally {
      setCommandBusy(false)
    }
  }

  const changeTrashMode = async (nextTrashMode: boolean) => {
    setTrashMode(nextTrashMode)
    await history.refresh({ deleted: nextTrashMode })
    await history.selectMeeting(null)
  }

  return (
    <div className="desktop-shell">
      <HistorySidebar
        meetings={history.meetings}
        selectedMeetingId={history.selectedMeetingId}
        trashMode={trashMode}
        loading={history.status === 'loading'}
        onSelect={(meetingId) => void history.selectMeeting(meetingId)}
        onOpenSettings={() => setSettingsOpen(true)}
        onTrashModeChange={(next) => void changeTrashMode(next)}
        onMoveToTrash={(meetingId) => void history.moveToTrash(meetingId)}
        onRestore={(meetingId) => void history.restore(meetingId)}
        onPermanentDelete={(meetingId) => {
          if (window.confirm('永久删除这场会议及其录音？此操作无法撤销。')) {
            void history.permanentlyDelete(meetingId)
          }
        }}
        onEmptyTrash={() => {
          if (window.confirm('永久删除回收站中的全部会议？')) {
            void history.emptyTrash()
          }
        }}
      />

      <section className="desktop-main">
        {history.error && <div className="global-error">{history.error}</div>}
        <MeetingWorkspace
          meeting={history.selectedMeeting}
          activeMeetingId={session.recording.meetingId}
          activeRecordingStatus={session.recording.status}
          transcription={session.transcription}
          minutes={session.minutes}
          tab={tab}
          onTabChange={setTab}
          onRetryTranscription={() => void session.retryTranscription()}
          onRetryMinutes={() => void session.retryMinutes()}
        />
        {!trashMode && (
          <RecordingBar
            status={session.recording.status}
            sources={sources}
            elapsedMs={elapsedMs}
            busy={commandBusy}
            onSourcesChange={setSources}
            onStart={() => void start()}
            onPause={() => void pause()}
            onResume={() => void resume()}
            onStop={() => void stop()}
          />
        )}
      </section>

      {settingsOpen && (
        <SettingsDialog settings={providerSettings} onClose={() => setSettingsOpen(false)} />
      )}
    </div>
  )
}

function defaultMeetingTitle() {
  return new Intl.DateTimeFormat('zh-CN', {
    month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit', hour12: false,
  }).format(new Date()) + ' 会议'
}
