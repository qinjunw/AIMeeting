import { Mic, MonitorSpeaker, Pause, Play, Square } from 'lucide-react'

import type { AudioSourceSelection } from '../../bridge/recordingClient'
import type { RecordingStatus } from '../../domain/meeting'

type RecordingBarProps = {
  status: RecordingStatus | 'idle'
  sources: AudioSourceSelection
  elapsedMs: number
  busy: boolean
  onSourcesChange: (sources: AudioSourceSelection) => void
  onStart: () => void
  onPause: () => void
  onResume: () => void
  onStop: () => void
}

export function RecordingBar({
  status,
  sources,
  elapsedMs,
  busy,
  onSourcesChange,
  onStart,
  onPause,
  onResume,
  onStop,
}: RecordingBarProps) {
  const active = status === 'recording'
  const paused = status === 'paused' || status === 'interrupted'
  const locked = active || busy

  return (
    <footer className="recording-bar">
      <div className="source-toggles" aria-label="录音来源">
        <SourceToggle
          label="麦克风"
          icon={<Mic aria-hidden="true" />}
          checked={sources.microphone}
          disabled={locked}
          onChange={(microphone) => onSourcesChange({ ...sources, microphone })}
        />
        <SourceToggle
          label="系统声音"
          icon={<MonitorSpeaker aria-hidden="true" />}
          checked={sources.systemAudio}
          disabled={locked}
          onChange={(systemAudio) => onSourcesChange({ ...sources, systemAudio })}
        />
      </div>

      <div className="recording-primary">
        {active ? (
          <button className="control-button control-button--secondary" type="button" onClick={onPause} disabled={busy}>
            <Pause aria-hidden="true" />
            暂停
          </button>
        ) : paused ? (
          <button className="control-button control-button--primary" type="button" onClick={onResume} disabled={busy || (!sources.microphone && !sources.systemAudio)}>
            <Play aria-hidden="true" />
            继续
          </button>
        ) : (
          <button className="control-button control-button--record" type="button" onClick={onStart} disabled={busy || (!sources.microphone && !sources.systemAudio)}>
            <span className="record-dot" aria-hidden="true" />
            开始录音
          </button>
        )}
        {(active || paused) && (
          <button className="icon-button icon-button--stop" type="button" onClick={onStop} disabled={busy} title="结束会议">
            <Square aria-hidden="true" />
          </button>
        )}
      </div>

      <div className="recording-clock" aria-label={`录音时长 ${formatDuration(elapsedMs)}`}>
        <span className={active ? 'status-pulse' : 'status-pulse status-pulse--paused'} />
        <strong>{formatDuration(elapsedMs)}</strong>
        <span>{statusLabel(status)}</span>
      </div>
    </footer>
  )
}

type SourceToggleProps = {
  label: string
  icon: React.ReactNode
  checked: boolean
  disabled: boolean
  onChange: (checked: boolean) => void
}

function SourceToggle({ label, icon, checked, disabled, onChange }: SourceToggleProps) {
  return (
    <label className={`source-toggle${checked ? ' source-toggle--checked' : ''}`}>
      <input
        type="checkbox"
        checked={checked}
        disabled={disabled}
        onChange={(event) => onChange(event.target.checked)}
      />
      {icon}
      <span>{label}</span>
    </label>
  )
}

function statusLabel(status: RecordingStatus | 'idle') {
  if (status === 'recording') return '正在录音'
  if (status === 'paused') return '已暂停'
  if (status === 'interrupted') return '录音已中断'
  if (status === 'processing') return '正在收尾'
  if (status === 'ready') return '已结束'
  return '准备就绪'
}

function formatDuration(milliseconds: number) {
  const totalSeconds = Math.floor(milliseconds / 1000)
  const hours = Math.floor(totalSeconds / 3600)
  const minutes = Math.floor((totalSeconds % 3600) / 60)
  const seconds = totalSeconds % 60
  return [hours, minutes, seconds].map((part) => String(part).padStart(2, '0')).join(':')
}
