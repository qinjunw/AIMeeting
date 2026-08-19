import { AudioWaveform, Check, Loader2, PlugZap, Radio, Save, Sparkles } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'

import type {
  ProviderCapability,
  ProviderEndpointFlavor,
  ProviderProfile,
  SaveProviderProfileRequest,
} from '../../bridge/providerClient'
import { Modal } from '../../components/Modal'
import type { useProviderSettings } from './useProviderSettings'

type SettingsDialogProps = {
  settings: ReturnType<typeof useProviderSettings>
  onClose: () => void
}

type ProviderDraft = {
  id: string
  capability: ProviderCapability
  name: string
  baseUrl: string
  model: string
  endpointFlavor: ProviderEndpointFlavor
  apiKey: string
}

const defaults: Record<ProviderCapability, ProviderDraft> = {
  live_transcription: {
    id: 'live-default', capability: 'live_transcription', name: '实时转写',
    baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode/v1',
    model: 'paraformer-realtime-v2', endpointFlavor: 'realtime-websocket', apiKey: '',
  },
  file_transcription: {
    id: 'file-default', capability: 'file_transcription', name: '录音文件转写',
    baseUrl: 'https://api.openai.com/v1', model: 'whisper-1',
    endpointFlavor: 'audio-transcriptions', apiKey: '',
  },
  minutes: {
    id: 'minutes-default', capability: 'minutes', name: '会议纪要',
    baseUrl: 'https://api.openai.com/v1', model: 'gpt-4.1-mini',
    endpointFlavor: 'chat-completions', apiKey: '',
  },
}

export function SettingsDialog({ settings, onClose }: SettingsDialogProps) {
  const [drafts, setDrafts] = useState(defaults)
  const [saving, setSaving] = useState<ProviderCapability | null>(null)
  const [saved, setSaved] = useState<ProviderCapability | null>(null)

  useEffect(() => {
    setDrafts((current) => ({
      live_transcription: mergeProfile(current.live_transcription, findDefault(settings.profiles, 'live_transcription')),
      file_transcription: mergeProfile(current.file_transcription, findDefault(settings.profiles, 'file_transcription')),
      minutes: mergeProfile(current.minutes, findDefault(settings.profiles, 'minutes')),
    }))
  }, [settings.profiles])

  const sections = useMemo(() => [
    { capability: 'live_transcription' as const, title: '实时语音转文字', description: '录音时显示低延迟字幕', icon: <Radio /> },
    { capability: 'file_transcription' as const, title: '录音文件转写', description: '实时转写失败后重新处理', icon: <AudioWaveform /> },
    { capability: 'minutes' as const, title: '会议纪要', description: '整理为简体中文纪要', icon: <Sparkles /> },
  ], [])

  const update = (capability: ProviderCapability, patch: Partial<ProviderDraft>) => {
    setDrafts((current) => ({ ...current, [capability]: { ...current[capability], ...patch } }))
    setSaved(null)
  }

  const save = async (capability: ProviderCapability) => {
    setSaving(capability)
    try {
      const draft = drafts[capability]
      const request: SaveProviderProfileRequest = {
        ...draft,
        apiKey: draft.apiKey.trim() || null,
        isDefault: true,
      }
      await settings.saveProfile(request)
      setSaved(capability)
      setDrafts((current) => ({
        ...current,
        [capability]: { ...current[capability], apiKey: '' },
      }))
    } finally {
      setSaving(null)
    }
  }

  return (
    <Modal title="设置" onClose={onClose} width="large">
      <div className="settings-stack">
        {sections.map((section) => {
          const draft = drafts[section.capability]
          const profile = findDefault(settings.profiles, section.capability)
          return (
            <section className="provider-section" key={section.capability}>
              <header>
                <span className="provider-icon">{section.icon}</span>
                <div><h3>{section.title}</h3><p>{section.description}</p></div>
                <span className={profile?.hasSecret ? 'config-state config-state--ready' : 'config-state'}>
                  {profile?.hasSecret ? '已配置' : '未配置'}
                </span>
              </header>
              <div className="provider-fields">
                <label><span>服务地址</span><input value={draft.baseUrl} onChange={(event) => update(section.capability, { baseUrl: event.target.value })} /></label>
                <label><span>模型</span><input value={draft.model} onChange={(event) => update(section.capability, { model: event.target.value })} /></label>
                <label className="provider-key-field"><span>API Key</span><input type="password" autoComplete="new-password" value={draft.apiKey} placeholder={profile?.hasSecret ? '已安全保存，留空则不修改' : '输入 API Key'} onChange={(event) => update(section.capability, { apiKey: event.target.value })} /></label>
                <div className="provider-actions">
                  <button className="secondary-button provider-save" type="button" disabled={saving !== null} onClick={() => void save(section.capability)}>
                    {saving === section.capability ? <Loader2 className="spin" /> : saved === section.capability ? <Check /> : <Save />}
                    {saved === section.capability ? '已保存' : '保存'}
                  </button>
                  <button className="secondary-button provider-test" type="button" disabled={!profile?.hasSecret || settings.testingProfileId !== null} onClick={() => profile && void settings.testProfile(profile.id)}>
                    {settings.testingProfileId === profile?.id ? <Loader2 className="spin" /> : <PlugZap />}
                    测试
                  </button>
                </div>
                {settings.testResult?.providerId === profile?.id && (
                  <span className="provider-test-result">{settings.testResult?.detail}</span>
                )}
              </div>
            </section>
          )
        })}
        {settings.error && <div className="status-banner status-banner--error">{settings.error}</div>}
      </div>
    </Modal>
  )
}

function findDefault(profiles: ProviderProfile[], capability: ProviderCapability) {
  return profiles.find((profile) => profile.capability === capability && profile.isDefault)
    ?? profiles.find((profile) => profile.capability === capability)
}

function mergeProfile(draft: ProviderDraft, profile: ProviderProfile | undefined): ProviderDraft {
  if (!profile) return draft
  return {
    ...draft,
    id: profile.id,
    name: profile.name,
    baseUrl: profile.baseUrl,
    model: profile.model,
    endpointFlavor: profile.endpointFlavor,
  }
}
