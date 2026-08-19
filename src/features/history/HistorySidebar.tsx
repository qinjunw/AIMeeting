import {
  ArchiveRestore,
  MoreHorizontal,
  Search,
  Settings,
  Trash2,
} from 'lucide-react'
import { useMemo, useState } from 'react'

import type { MeetingSummary } from '../../bridge/meetingRepositoryClient'

type HistorySidebarProps = {
  meetings: MeetingSummary[]
  selectedMeetingId: string | null
  trashMode: boolean
  loading: boolean
  onSelect: (meetingId: string) => void
  onOpenSettings: () => void
  onTrashModeChange: (trashMode: boolean) => void
  onMoveToTrash: (meetingId: string) => void
  onRestore: (meetingId: string) => void
  onPermanentDelete: (meetingId: string) => void
  onEmptyTrash: () => void
}

export function HistorySidebar({
  meetings,
  selectedMeetingId,
  trashMode,
  loading,
  onSelect,
  onOpenSettings,
  onTrashModeChange,
  onMoveToTrash,
  onRestore,
  onPermanentDelete,
  onEmptyTrash,
}: HistorySidebarProps) {
  const [query, setQuery] = useState('')
  const [menuMeetingId, setMenuMeetingId] = useState<string | null>(null)
  const visibleMeetings = useMemo(() => {
    const normalized = query.trim().toLocaleLowerCase('zh-CN')
    if (!normalized) return meetings
    return meetings.filter((meeting) =>
      meeting.title.toLocaleLowerCase('zh-CN').includes(normalized),
    )
  }, [meetings, query])

  return (
    <aside className="history-sidebar">
      <header className="app-brand">
        <div className="brand-mark" aria-hidden="true"><span /></div>
        <div>
          <strong>AI Meeting</strong>
          <span>会议记录</span>
        </div>
        <button className="icon-button icon-button--inverse" type="button" onClick={onOpenSettings} title="设置">
          <Settings aria-hidden="true" />
        </button>
      </header>

      <div className="history-search">
        <Search aria-hidden="true" />
        <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="搜索会议" aria-label="搜索会议" />
      </div>

      <div className="history-heading">
        <span>{trashMode ? '回收站' : '最近会议'}</span>
        <span>{meetings.length}</span>
      </div>

      <div className="history-list" aria-busy={loading}>
        {visibleMeetings.map((meeting) => (
          <div
            className={`history-item${selectedMeetingId === meeting.id ? ' history-item--selected' : ''}`}
            key={meeting.id}
          >
            <button className="history-item__main" type="button" onClick={() => onSelect(meeting.id)}>
              <strong>{meeting.title}</strong>
              <span>{formatMeetingTime(meeting.createdAt)}</span>
              <small>{meetingState(meeting)}</small>
            </button>
            <button
              className="history-item__menu"
              type="button"
              onClick={() => setMenuMeetingId((current) => current === meeting.id ? null : meeting.id)}
              title="更多操作"
            >
              <MoreHorizontal aria-hidden="true" />
            </button>
            {menuMeetingId === meeting.id && (
              <div className="context-menu">
                {trashMode ? (
                  <>
                    <button type="button" onClick={() => { onRestore(meeting.id); setMenuMeetingId(null) }}><ArchiveRestore />恢复</button>
                    <button className="danger-action" type="button" onClick={() => { onPermanentDelete(meeting.id); setMenuMeetingId(null) }}><Trash2 />永久删除</button>
                  </>
                ) : (
                  <button className="danger-action" type="button" onClick={() => { onMoveToTrash(meeting.id); setMenuMeetingId(null) }}><Trash2 />移到回收站</button>
                )}
              </div>
            )}
          </div>
        ))}
        {!loading && visibleMeetings.length === 0 && (
          <div className="history-empty">{query ? '没有匹配的会议' : trashMode ? '回收站为空' : '还没有会议记录'}</div>
        )}
      </div>

      <div className="sidebar-footer">
        <button className="sidebar-mode-button" type="button" onClick={() => onTrashModeChange(!trashMode)}>
          {trashMode ? <ArchiveRestore aria-hidden="true" /> : <Trash2 aria-hidden="true" />}
          {trashMode ? '返回会议' : '回收站'}
        </button>
        {trashMode && meetings.length > 0 && (
          <button className="text-button text-button--danger" type="button" onClick={onEmptyTrash}>清空</button>
        )}
      </div>
    </aside>
  )
}

function formatMeetingTime(value: string) {
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return value
  return new Intl.DateTimeFormat('zh-CN', {
    month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit', hour12: false,
  }).format(date)
}

function meetingState(meeting: MeetingSummary) {
  if (meeting.recordingStatus === 'recording') return '正在录音'
  if (meeting.recordingStatus === 'paused') return '已暂停'
  if (meeting.transcriptionStatus === 'failed') return '可重新转写'
  if (meeting.minutesStatus === 'processing') return '正在整理'
  if (meeting.minutesStatus === 'ready') return '纪要已完成'
  return '录音已保存'
}
