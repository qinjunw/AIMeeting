import { FileText, RotateCcw, ScrollText } from 'lucide-react'

import type { MeetingDetails } from '../../bridge/meetingRepositoryClient'
import type { MinutesViewState, TranscriptionViewState } from './useMeetingSession'

export type WorkspaceTab = 'minutes' | 'transcript'

type MeetingWorkspaceProps = {
  meeting: MeetingDetails | null
  activeMeetingId: string | null
  transcription: TranscriptionViewState
  minutes: MinutesViewState
  tab: WorkspaceTab
  onTabChange: (tab: WorkspaceTab) => void
  onRetryTranscription: () => void
  onRetryMinutes: () => void
}

export function MeetingWorkspace({
  meeting,
  activeMeetingId,
  transcription,
  minutes,
  tab,
  onTabChange,
  onRetryTranscription,
  onRetryMinutes,
}: MeetingWorkspaceProps) {
  const isActiveMeeting = activeMeetingId !== null && activeMeetingId === meeting?.id
  const liveTranscript = [
    ...transcription.segments.map((segment) => segment.text),
    transcription.interimText,
  ].filter(Boolean).join('\n')
  const transcript = isActiveMeeting && liveTranscript ? liveTranscript : meeting?.transcript ?? ''
  const minutesContent = isActiveMeeting && minutes.content ? minutes.content : meeting?.minutes?.content ?? ''
  const transcriptionStatus = isActiveMeeting ? transcription.status : meeting?.transcriptionStatus
  const minutesStatus = isActiveMeeting ? minutes.status : meeting?.minutesStatus

  return (
    <main className="meeting-workspace">
      <header className="workspace-header">
        <div>
          <span className="workspace-eyebrow">{meeting ? formatDate(meeting.createdAt) : '新会议'}</span>
          <h1>{meeting?.title ?? '准备开始一场会议'}</h1>
        </div>
        {meeting && <span className={`meeting-status meeting-status--${meeting.recordingStatus}`}>{recordingLabel(meeting.recordingStatus)}</span>}
      </header>

      {(transcription.error || minutes.error) && isActiveMeeting && (
        <div className="status-banner status-banner--warning">
          <span>{transcription.error ?? minutes.error}</span>
          {transcription.error ? <button type="button" onClick={onRetryTranscription}>重新转写</button> : <button type="button" onClick={onRetryMinutes}>重试纪要</button>}
        </div>
      )}

      <div className="workspace-tabs" role="tablist" aria-label="会议内容">
        <button className={tab === 'minutes' ? 'workspace-tab workspace-tab--active' : 'workspace-tab'} type="button" role="tab" aria-selected={tab === 'minutes'} onClick={() => onTabChange('minutes')}>
          <FileText aria-hidden="true" />会议纪要
        </button>
        <button className={tab === 'transcript' ? 'workspace-tab workspace-tab--active' : 'workspace-tab'} type="button" role="tab" aria-selected={tab === 'transcript'} onClick={() => onTabChange('transcript')}>
          <ScrollText aria-hidden="true" />完整转写
        </button>
      </div>

      <section className="workspace-content" role="tabpanel">
        {tab === 'minutes' ? (
          minutesContent ? (
            <article className="document-content">{minutesContent}</article>
          ) : (
            <EmptyDocument
              icon={<FileText aria-hidden="true" />}
              title={minutesStatus === 'processing' ? '正在整理会议纪要' : '会议纪要将在转写后生成'}
              action={meeting && minutesStatus === 'failed' ? <button className="secondary-button" type="button" onClick={onRetryMinutes}><RotateCcw />重试纪要</button> : undefined}
            />
          )
        ) : transcript ? (
          <article className="document-content document-content--transcript">{transcript}</article>
        ) : (
          <EmptyDocument
            icon={<ScrollText aria-hidden="true" />}
            title={transcriptionStatus === 'streaming' ? '正在聆听' : transcriptionStatus === 'failed' ? '录音已保存，转写暂不可用' : '转写内容会显示在这里'}
            action={meeting && transcriptionStatus === 'failed' ? <button className="secondary-button" type="button" onClick={onRetryTranscription}><RotateCcw />重新转写</button> : undefined}
          />
        )}
      </section>
    </main>
  )
}

function EmptyDocument({ icon, title, action }: { icon: React.ReactNode; title: string; action?: React.ReactNode }) {
  return <div className="document-empty"><span>{icon}</span><p>{title}</p>{action}</div>
}

function formatDate(value: string) {
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return value
  return new Intl.DateTimeFormat('zh-CN', {
    year: 'numeric', month: 'long', day: 'numeric', weekday: 'short',
  }).format(date)
}

function recordingLabel(status: MeetingDetails['recordingStatus']) {
  if (status === 'recording') return '正在录音'
  if (status === 'paused') return '已暂停'
  if (status === 'interrupted') return '意外中断'
  if (status === 'processing') return '正在处理'
  return '已保存'
}
