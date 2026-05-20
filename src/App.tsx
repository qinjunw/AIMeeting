import { useEffect, useMemo, useRef, useState } from 'react'
import type { FormEvent } from 'react'
import type { LucideIcon } from 'lucide-react'
import {
  Activity,
  Bot,
  Clock,
  Database,
  Eye,
  FileText,
  Globe2,
  Keyboard,
  Languages,
  ListChecks,
  Loader2,
  Mic,
  Monitor,
  Pause,
  Plus,
  Radio,
  Search,
  Send,
  Settings,
  Shield,
  Sparkles,
  Square,
  Trash2,
  X,
} from 'lucide-react'
import { formatClock, formatDuration, makeId } from './lib/time'
import { loadJson, saveJson } from './lib/storage'
import { probeMicrophone, probeSystemAudio } from './services/audioCapture'
import {
  cancelStreamingAsrSession,
  finishStreamingAsrSession,
  listenStreamingAsrEvents,
  pushStreamingAsrAudio,
  startStreamingAsrSession,
} from './services/asrTranscription'
import { normalizeTranscriptText } from './services/chineseText'
import { generateAgentDraft, generateMeetingDigest } from './services/modelProvider'
import { runAutoSearch } from './services/searchTool'
import type {
  AgentResponse,
  AsrProviderConfig,
  AudioSource,
  CaptureProbe,
  MeetingDigest,
  MeetingMode,
  MeetingRecord,
  MeetingSegment,
  ProviderConfig,
  SearchConfig,
  SearchTrace,
  StreamingAsrEvent,
  TranscriptionStatus,
  VoiceTrigger,
} from './types'

const providerStorageKey = 'aimeeting.provider'
const asrProviderStorageKey = 'aimeeting.asrProvider'
const searchStorageKey = 'aimeeting.search'
const segmentsStorageKey = 'aimeeting.v2.segments'
const responsesStorageKey = 'aimeeting.v2.responses'
const meetingsStorageKey = 'aimeeting.v3.meetings'
const activeMeetingIdStorageKey = 'aimeeting.v3.activeMeetingId'
const activeMeetingCreatedAtStorageKey = 'aimeeting.v3.activeMeetingCreatedAt'
const speechLangStorageKey = 'aimeeting.speechLang'
const wakePhrasesStorageKey = 'aimeeting.wakePhrases'
const digestStorageKey = 'aimeeting.v1.digest'

const defaultProvider: ProviderConfig = {
  baseUrl: 'https://api.openai.com/v1',
  apiKey: '',
  model: 'gpt-4.1-mini',
  endpointFlavor: 'chat-completions',
  temperature: 0.25,
}

const defaultAsrProvider: AsrProviderConfig = {
  baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode/v1',
  apiKey: '',
  model: 'paraformer-realtime-v2',
}

const defaultSearch: SearchConfig = {
  mode: 'auto',
  endpointTemplate: '',
  redactBeforeSearch: false,
}

const modeMeta: Record<MeetingMode, { label: string; tone: string }> = {
  recording: { label: 'Recording', tone: 'green' },
  dialogue: { label: 'Dialogue', tone: 'blue' },
  searching: { label: 'Searching', tone: 'violet' },
  paused: { label: 'Paused', tone: 'muted' },
}

const speakerOptions = ['Speaker A', 'Speaker B', 'Speaker C', 'Me']
const sourceOptions: AudioSource[] = ['system', 'microphone', 'mixed']
const streamingSampleRate = 16000
const streamingFrameMs = 100
const streamingRemainderMinMs = 30
const wakeSilenceMs = 4000
const digestUpdateMs = 1200

const emptyDigest: MeetingDigest = {
  text: '',
  updatedAt: '',
  providerLabel: '',
  segmentCount: 0,
}

type DigestStatus = 'idle' | 'pending' | 'updating' | 'error'

type AsrActivity = {
  queued: number
  inFlight: number
  lastLatencyMs: number
}

type WakeCapture = {
  phrase: string
  transcript: string
  parts: string[]
  startedAt: string
}

type StreamingAsrSessionInfo = {
  sessionId: string
  meetingId: string
  recordingRunId: string
  providerLabel: string
  startedAt: number
  pendingFrames: number
  droppedFrames: number
  lastLatencyMs: number
  firstInterimAt?: number
  firstFinalAt?: number
  pushErrorReported?: boolean
}

type WindowWithAudioContext = Window & {
  webkitAudioContext?: typeof AudioContext
}

function App() {
  const [mode, setMode] = useState<MeetingMode>('paused')
  const [activeMeetingId, setActiveMeetingId] = useState(() => loadJson(activeMeetingIdStorageKey, makeId('meeting')))
  const [activeMeetingCreatedAt, setActiveMeetingCreatedAt] = useState(() =>
    loadJson(activeMeetingCreatedAtStorageKey, new Date().toISOString()),
  )
  const [segments, setSegments] = useState<MeetingSegment[]>(() => loadJson(segmentsStorageKey, []))
  const [responses, setResponses] = useState<AgentResponse[]>(() => loadJson(responsesStorageKey, []))
  const [meetings, setMeetings] = useState<MeetingRecord[]>(() => loadJson(meetingsStorageKey, []))
  const [selectedHistoryId, setSelectedHistoryId] = useState<string | null>(null)
  const [checkedHistoryIds, setCheckedHistoryIds] = useState<string[]>([])
  const [provider, setProvider] = useState<ProviderConfig>(() => ({
    ...loadJson(providerStorageKey, defaultProvider),
    apiKey: '',
  }))
  const [asrProvider, setAsrProvider] = useState<AsrProviderConfig>(() => ({
    ...loadJson(asrProviderStorageKey, defaultAsrProvider),
    apiKey: '',
  }))
  const [searchConfig, setSearchConfig] = useState<SearchConfig>(() => loadJson(searchStorageKey, defaultSearch))
  const [question, setQuestion] = useState('')
  const [manualText, setManualText] = useState('')
  const [manualSpeaker, setManualSpeaker] = useState('Me')
  const [manualSource, setManualSource] = useState<AudioSource>('microphone')
  const [captureLog, setCaptureLog] = useState<CaptureProbe[]>([])
  const [showEvidence, setShowEvidence] = useState(false)
  const [showAdvancedSettings, setShowAdvancedSettings] = useState(false)
  const [showHistoryDetails, setShowHistoryDetails] = useState(false)
  const [isThinking, setIsThinking] = useState(false)
  const [speechStatus, setSpeechStatus] = useState<TranscriptionStatus>('idle')
  const [speechLang, setSpeechLang] = useState(() => loadJson(speechLangStorageKey, 'zh-CN'))
  const [wakePhrases, setWakePhrases] = useState(() => loadJson(wakePhrasesStorageKey, '嗨助手,嘿助手,助手,hey assistant'))
  const [interimTranscript, setInterimTranscript] = useState('')
  const [lastVoiceTrigger, setLastVoiceTrigger] = useState<VoiceTrigger | null>(null)
  const [autoAskOnWake, setAutoAskOnWake] = useState(false)
  const [meetingDigest, setMeetingDigest] = useState<MeetingDigest>(() => loadJson(digestStorageKey, emptyDigest))
  const [digestStatus, setDigestStatus] = useState<DigestStatus>('idle')
  const [asrActivity, setAsrActivity] = useState<AsrActivity>({ queued: 0, inFlight: 0, lastLatencyMs: 0 })

  const activeMeetingIdRef = useRef(activeMeetingId)
  const activeMeetingCreatedAtRef = useRef(activeMeetingCreatedAt)
  const segmentsRef = useRef(segments)
  const responsesRef = useRef(responses)
  const meetingsRef = useRef(meetings)
  const providerRef = useRef(provider)
  const meetingDigestRef = useRef(meetingDigest)
  const isThinkingRef = useRef(false)
  const autoAskOnWakeRef = useRef(autoAskOnWake)
  const mediaStreamRef = useRef<MediaStream | null>(null)
  const audioContextRef = useRef<AudioContext | null>(null)
  const audioSourceRef = useRef<MediaStreamAudioSourceNode | null>(null)
  const audioProcessorRef = useRef<ScriptProcessorNode | null>(null)
  const pcmBufferRef = useRef<Float32Array[]>([])
  const pcmSampleCountRef = useRef(0)
  const asrRecordingRef = useRef(false)
  const streamingSessionsRef = useRef<Map<string, StreamingAsrSessionInfo>>(new Map())
  const currentStreamingSessionIdRef = useRef<string | null>(null)
  const streamingStartTokenRef = useRef(0)
  const streamingAudioPushChainsRef = useRef<Map<string, Promise<void>>>(new Map())
  const asrPendingCountRef = useRef<Map<string, number>>(new Map())
  const asrIdleResolversRef = useRef<Map<string, Array<() => void>>>(new Map())
  const streamingEventHandlerRef = useRef<(event: StreamingAsrEvent) => void>(() => undefined)
  const wakeCaptureRef = useRef<WakeCapture | null>(null)
  const wakeSilenceTimerRef = useRef<number | null>(null)
  const digestTimerRef = useRef<number | null>(null)
  const digestRunningRef = useRef(false)
  const digestDirtyRef = useRef(false)

  const latestResponse = responses[0]
  const durationMs = segments.at(-1)?.endMs ?? 0
  const sourceCounts = useMemo(
    () =>
      sourceOptions.map((source) => ({
        source,
        count: segments.filter((segment) => segment.source === source).length,
      })),
    [segments],
  )
  const selectedHistory = useMemo(
    () => meetings.find((meeting) => meeting.id === selectedHistoryId) ?? null,
    [meetings, selectedHistoryId],
  )
  const checkedHistoryIdSet = useMemo(() => new Set(checkedHistoryIds), [checkedHistoryIds])

  useEffect(() => {
    activeMeetingIdRef.current = activeMeetingId
    saveJson(activeMeetingIdStorageKey, activeMeetingId)
  }, [activeMeetingId])

  useEffect(() => {
    activeMeetingCreatedAtRef.current = activeMeetingCreatedAt
    saveJson(activeMeetingCreatedAtStorageKey, activeMeetingCreatedAt)
  }, [activeMeetingCreatedAt])

  useEffect(() => {
    segmentsRef.current = segments
    saveJson(segmentsStorageKey, segments)
    scheduleDigestUpdate()
  }, [segments])

  useEffect(() => {
    responsesRef.current = responses
    saveJson(responsesStorageKey, responses)
  }, [responses])

  useEffect(() => {
    meetingsRef.current = meetings
    saveJson(meetingsStorageKey, meetings)
  }, [meetings])

  useEffect(() => {
    const availableIds = new Set(meetings.map((meeting) => meeting.id))
    setCheckedHistoryIds((current) => current.filter((id) => availableIds.has(id)))
  }, [meetings])

  useEffect(() => {
    if (!selectedHistory) {
      setShowHistoryDetails(false)
    }
  }, [selectedHistory])

  useEffect(() => {
    saveJson(searchStorageKey, searchConfig)
  }, [searchConfig])

  useEffect(() => {
    providerRef.current = provider
    saveJson(providerStorageKey, { ...provider, apiKey: '' })
    scheduleDigestUpdate()
  }, [provider])

  useEffect(() => {
    saveJson(asrProviderStorageKey, { ...asrProvider, apiKey: '' })
  }, [asrProvider])

  useEffect(() => {
    saveJson(speechLangStorageKey, speechLang)
  }, [speechLang])

  useEffect(() => {
    saveJson(wakePhrasesStorageKey, wakePhrases)
  }, [wakePhrases])

  useEffect(() => {
    meetingDigestRef.current = meetingDigest
    saveJson(digestStorageKey, meetingDigest)
  }, [meetingDigest])

  useEffect(() => {
    autoAskOnWakeRef.current = autoAskOnWake
  }, [autoAskOnWake])

  useEffect(() => {
    streamingEventHandlerRef.current = handleStreamingAsrEvent
  })

  useEffect(() => {
    let unlisten: (() => void) | undefined
    let disposed = false

    try {
      listenStreamingAsrEvents((event) => streamingEventHandlerRef.current(event))
        .then((nextUnlisten) => {
          if (disposed) {
            nextUnlisten()
            return
          }
          unlisten = nextUnlisten
        })
        .catch((error) => {
          setCaptureLog((current) => [
            {
              ok: false,
              label: 'Streaming ASR listener failed',
              detail: error instanceof Error ? error.message : '无法监听实时 ASR 事件。',
            },
            ...current,
          ].slice(0, 4))
        })
    } catch {
      return
    }

    return () => {
      disposed = true
      unlisten?.()
    }
  }, [])

  useEffect(() => {
    if (!showAdvancedSettings && !showHistoryDetails) {
      return
    }

    function closeOnEscape(event: KeyboardEvent) {
      if (event.key === 'Escape') {
        setShowAdvancedSettings(false)
        setShowHistoryDetails(false)
      }
    }

    window.addEventListener('keydown', closeOnEscape)
    return () => window.removeEventListener('keydown', closeOnEscape)
  }, [showAdvancedSettings, showHistoryDetails])

  useEffect(
    () => () => {
      cancelCurrentStreamingAsr()
      clearWakeSilenceTimer()
      clearDigestTimer()
    },
    [],
  )

  function setMeetingsState(updater: (current: MeetingRecord[]) => MeetingRecord[]) {
    setMeetings((current) => {
      const nextMeetings = updater(current)
      meetingsRef.current = nextMeetings
      saveJson(meetingsStorageKey, nextMeetings)
      return nextMeetings
    })
  }

  function updateHistoricalMeeting(meetingId: string, updater: (meeting: MeetingRecord) => MeetingRecord) {
    setMeetingsState((current) => current.map((meeting) => (meeting.id === meetingId ? updater(meeting) : meeting)))
  }

  function toggleHistoryChecked(meetingId: string) {
    setCheckedHistoryIds((current) =>
      current.includes(meetingId) ? current.filter((id) => id !== meetingId) : [...current, meetingId],
    )
  }

  function toggleAllHistoryChecked() {
    setCheckedHistoryIds((current) => (current.length === meetings.length ? [] : meetings.map((meeting) => meeting.id)))
  }

  function deleteCheckedHistory() {
    const idsToDelete = new Set(checkedHistoryIds)
    if (idsToDelete.size === 0) {
      return
    }
    if (!window.confirm(`确定删除选中的 ${idsToDelete.size} 个历史 session？此操作会同步清理持久化记录。`)) {
      return
    }

    setMeetingsState((current) => current.filter((meeting) => !idsToDelete.has(meeting.id)))
    setSelectedHistoryId((current) => (current && idsToDelete.has(current) ? null : current))
    setCheckedHistoryIds([])
  }

  function deleteAllHistory() {
    if (meetings.length === 0) {
      return
    }
    if (!window.confirm(`确定删除全部 ${meetings.length} 个历史 session？此操作会同步清理持久化记录。`)) {
      return
    }

    setMeetingsState(() => [])
    setSelectedHistoryId(null)
    setCheckedHistoryIds([])
  }

  function resetActiveMeeting(nextMode: MeetingMode = 'paused') {
    const nextMeetingId = makeId('meeting')
    const createdAt = new Date().toISOString()

    activeMeetingIdRef.current = nextMeetingId
    activeMeetingCreatedAtRef.current = createdAt
    segmentsRef.current = []
    responsesRef.current = []
    meetingDigestRef.current = emptyDigest
    setActiveMeetingId(nextMeetingId)
    setActiveMeetingCreatedAt(createdAt)
    setSegments([])
    setResponses([])
    setMeetingDigest(emptyDigest)
    setDigestStatus('idle')
    setInterimTranscript('')
    setLastVoiceTrigger(null)
    setQuestion('')
    setMode(nextMode)
    setSpeechStatus('idle')
    setAsrActivity({ queued: 0, inFlight: 0, lastLatencyMs: 0 })
  }

  function cancelWakeCapture() {
    clearWakeSilenceTimer()
    wakeCaptureRef.current = null
    setLastVoiceTrigger(null)
  }

  function archiveActiveMeeting(meetingId: string) {
    const now = new Date().toISOString()
    const record: MeetingRecord = {
      id: meetingId,
      title: makeMeetingTitle(meetingDigestRef.current, segmentsRef.current, activeMeetingCreatedAtRef.current),
      status: 'finalizing',
      segments: segmentsRef.current,
      digest: meetingDigestRef.current,
      responses: responsesRef.current,
      createdAt: activeMeetingCreatedAtRef.current,
      updatedAt: now,
      stoppedAt: now,
    }

    setMeetingsState((current) => [record, ...current.filter((meeting) => meeting.id !== meetingId)].slice(0, 12))
    setSelectedHistoryId(meetingId)
    void finalizeArchivedMeeting(meetingId)
  }

  function pauseActiveMeeting() {
    const meetingId = activeMeetingIdRef.current
    stopStreamingAsr(meetingId)
    cancelWakeCapture()
    setQuestion('')
    setMode('paused')
  }

  function clearMeeting() {
    stopStreamingAsr(activeMeetingIdRef.current, { flushFinal: false })
    clearWakeSilenceTimer()
    wakeCaptureRef.current = null
    clearDigestTimer()
    asrPendingCountRef.current.delete(activeMeetingIdRef.current)
    asrIdleResolversRef.current.delete(activeMeetingIdRef.current)
    meetingDigestRef.current = emptyDigest
    segmentsRef.current = []
    responsesRef.current = []
    setSegments([])
    setResponses([])
    setMeetingDigest(emptyDigest)
    setDigestStatus('idle')
    setInterimTranscript('')
    setLastVoiceTrigger(null)
    setQuestion('')
    setSpeechStatus('idle')
    setMode('paused')
  }

  function addManualSegment(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const text = manualText.trim()

    if (!text) {
      return
    }

    handleRecognizedTranscript(text, 0.86, manualSpeaker, manualSource, { finishWakeImmediately: true })
    setManualText('')
  }

  async function runAgentQuestion(
    trimmed: string,
    contextSegments = segmentsRef.current,
    meetingId = activeMeetingIdRef.current,
  ) {
    if (!trimmed || isThinkingRef.current) {
      return
    }

    const providerConfig = providerRef.current
    if (!hasTextProvider(providerConfig)) {
      setCaptureLog((current) => [
        {
          ok: false,
          label: 'Agent Provider missing',
          detail: '请先完整配置 Agent Provider 的 Base URL、Model 和 API key。',
        },
        ...current,
      ].slice(0, 4))
      return
    }

    isThinkingRef.current = true
    setIsThinking(true)
    setMode('searching')

    const started = performance.now()
    const searches = await runAutoSearch(trimmed, contextSegments, searchConfig)
    const draft = await generateAgentDraft({ question: trimmed, segments: contextSegments, searches, provider: providerConfig })
    const response: AgentResponse = {
      id: makeId('answer'),
      question: trimmed,
      answer: draft.answer,
      planItems: draft.planItems,
      evidence: draft.evidence,
      searches,
      providerLabel: draft.providerLabel,
      latencyMs: Math.round(performance.now() - started),
      createdAt: new Date().toISOString(),
      error: draft.error,
    }

    if (meetingId === activeMeetingIdRef.current) {
      setResponses((current) => [response, ...current].slice(0, 8))
      setQuestion('')
      setShowEvidence(false)
      setMode('dialogue')
    } else {
      updateHistoricalMeeting(meetingId, (meeting) => ({
        ...meeting,
        responses: [response, ...meeting.responses].slice(0, 8),
        updatedAt: new Date().toISOString(),
      }))
    }
    setIsThinking(false)
    isThinkingRef.current = false
  }

  async function askAgent(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    await runAgentQuestion(question.trim())
  }

  async function probe(kind: 'mic' | 'system') {
    const result = kind === 'mic' ? await probeMicrophone() : await probeSystemAudio()
    setCaptureLog((current) => [result, ...current].slice(0, 4))
  }

  function hasTextProvider(config = providerRef.current) {
    return Boolean(config.apiKey.trim() && config.baseUrl.trim() && config.model.trim())
  }

  function hasAsrProvider(config = asrProvider) {
    return Boolean(config.apiKey.trim() && config.baseUrl.trim() && config.model.trim())
  }

  function providerConfigurationIssues() {
    const issues: string[] = []
    if (!hasAsrProvider(asrProvider)) {
      issues.push('ASR Provider')
    }
    if (!hasTextProvider(provider)) {
      issues.push('Agent Provider')
    }
    return issues
  }

  function clearDigestTimer() {
    if (digestTimerRef.current) {
      window.clearTimeout(digestTimerRef.current)
      digestTimerRef.current = null
    }
  }

  function scheduleDigestUpdate(delayMs = digestUpdateMs) {
    if (digestRunningRef.current) {
      digestDirtyRef.current = true
      return
    }

    if (digestTimerRef.current) {
      return
    }

    const currentDigest = meetingDigestRef.current
    if (!hasTextProvider() || segmentsRef.current.length === 0 || segmentsRef.current.length <= currentDigest.segmentCount) {
      return
    }

    setDigestStatus('pending')
    digestTimerRef.current = window.setTimeout(() => {
      digestTimerRef.current = null
      void runDigestUpdate()
    }, delayMs)
  }

  async function runDigestUpdate() {
    if (digestRunningRef.current) {
      return
    }

    const meetingId = activeMeetingIdRef.current
    const providerConfig = providerRef.current
    if (!hasTextProvider(providerConfig)) {
      setDigestStatus('idle')
      return
    }

    const currentDigest = meetingDigestRef.current
    const allSegments = segmentsRef.current
    const segmentStart = Math.min(currentDigest.segmentCount, allSegments.length)
    const newSegments = allSegments.slice(segmentStart)
    if (newSegments.length === 0) {
      setDigestStatus('idle')
      return
    }

    digestRunningRef.current = true
    digestDirtyRef.current = false
    setDigestStatus('updating')
    const result = await generateMeetingDigest({
      previousDigest: currentDigest.text,
      newSegments,
      provider: providerConfig,
    })

    const nextDigest: MeetingDigest = result.error
      ? {
          ...currentDigest,
          updatedAt: new Date().toISOString(),
          providerLabel: result.providerLabel,
          error: result.error,
        }
      : {
          text: result.digest,
          updatedAt: new Date().toISOString(),
          providerLabel: result.providerLabel,
          segmentCount: allSegments.length,
        }

    if (meetingId === activeMeetingIdRef.current) {
      meetingDigestRef.current = nextDigest
      setMeetingDigest(nextDigest)
      setDigestStatus(result.error ? 'error' : 'idle')
    } else {
      updateHistoricalMeeting(meetingId, (meeting) => {
        if (!result.error && nextDigest.segmentCount < meeting.digest.segmentCount) {
          return meeting
        }

        return {
          ...meeting,
          digest: nextDigest,
          title: makeMeetingTitle(nextDigest, meeting.segments, meeting.createdAt),
          updatedAt: new Date().toISOString(),
        }
      })
    }
    digestRunningRef.current = false

    if (
      meetingId === activeMeetingIdRef.current &&
      !result.error &&
      (digestDirtyRef.current || segmentsRef.current.length > nextDigest.segmentCount)
    ) {
      digestDirtyRef.current = false
      scheduleDigestUpdate(250)
    }
  }

  async function finalizeArchivedMeeting(meetingId: string) {
    await waitForAsrIdle(meetingId)

    const meeting = meetingsRef.current.find((item) => item.id === meetingId)
    if (!meeting) {
      return
    }

    const providerConfig = providerRef.current
    const segmentStart = Math.min(meeting.digest.segmentCount, meeting.segments.length)
    const newSegments = meeting.segments.slice(segmentStart)

    if (!hasTextProvider(providerConfig) || newSegments.length === 0) {
      updateHistoricalMeeting(meetingId, (current) => ({
        ...current,
        status: 'archived',
        updatedAt: new Date().toISOString(),
      }))
      return
    }

    const result = await generateMeetingDigest({
      previousDigest: meeting.digest.text,
      newSegments,
      provider: providerConfig,
    })

    const nextDigest: MeetingDigest = result.error
      ? {
          ...meeting.digest,
          updatedAt: new Date().toISOString(),
          providerLabel: result.providerLabel,
          error: result.error,
        }
      : {
          text: result.digest,
          updatedAt: new Date().toISOString(),
          providerLabel: result.providerLabel,
          segmentCount: meeting.segments.length,
        }

    updateHistoricalMeeting(meetingId, (current) => ({
      ...current,
      status: result.error ? 'error' : 'archived',
      digest: nextDigest,
      title: makeMeetingTitle(nextDigest, current.segments, current.createdAt),
      updatedAt: new Date().toISOString(),
      error: result.error,
    }))
  }

  function clearWakeSilenceTimer() {
    if (wakeSilenceTimerRef.current) {
      window.clearTimeout(wakeSilenceTimerRef.current)
      wakeSilenceTimerRef.current = null
    }
  }

  function scheduleWakeCaptureSilence() {
    clearWakeSilenceTimer()
    wakeSilenceTimerRef.current = window.setTimeout(() => {
      if ((asrPendingCountRef.current.get(activeMeetingIdRef.current) ?? 0) > 0) {
        scheduleWakeCaptureSilence()
        return
      }
      finalizeWakeCapture()
    }, wakeSilenceMs)
  }

  function startWakeCapture(trigger: VoiceTrigger) {
    wakeCaptureRef.current = {
      phrase: trigger.phrase,
      transcript: trigger.transcript,
      parts: [],
      startedAt: new Date().toISOString(),
    }
    setLastVoiceTrigger({ ...trigger, question: '' })
    setQuestion('')
    setMode('dialogue')
    setInterimTranscript(`已捕获触发词“${trigger.phrase}”，继续说你的要求。`)
    scheduleWakeCaptureSilence()
  }

  function appendWakeCommandText(text: string) {
    const capture = wakeCaptureRef.current
    if (!capture) {
      return
    }

    const cleanText = normalizeTranscriptText(text)
    if (cleanText) {
      capture.parts.push(cleanText)
    }

    const command = buildWakeCommand(capture)
    setQuestion(command)
    setLastVoiceTrigger({
      phrase: capture.phrase,
      transcript: capture.transcript,
      question: command,
      beforeText: '',
    })
    setInterimTranscript(command ? `正在采集助手请求：${command}` : `已捕获触发词“${capture.phrase}”，继续说你的要求。`)
    scheduleWakeCaptureSilence()
  }

  function finalizeWakeCapture() {
    const capture = wakeCaptureRef.current
    if (!capture) {
      return
    }

    clearWakeSilenceTimer()
    wakeCaptureRef.current = null
    const command = buildWakeCommand(capture) || '请基于当前会议纪要总结重点、待办事项和风险。'
    setQuestion(command)
    setLastVoiceTrigger({
      phrase: capture.phrase,
      transcript: capture.transcript,
      question: command,
      beforeText: '',
    })
    setInterimTranscript('')
    setMode('dialogue')

    if (autoAskOnWakeRef.current) {
      void runAgentQuestion(command, segmentsRef.current, activeMeetingIdRef.current)
    }
  }

  function handleRecognizedTranscript(
    rawText: string,
    confidence: number,
    speakerLabel: string,
    source: AudioSource,
    options: { finishWakeImmediately?: boolean } = {},
  ): MeetingSegment[] {
    const cleanText = normalizeTranscriptText(rawText)
    if (!cleanText) {
      return segmentsRef.current
    }

    if (wakeCaptureRef.current) {
      appendWakeCommandText(cleanText)
      if (options.finishWakeImmediately) {
        finalizeWakeCapture()
      }
      return segmentsRef.current
    }

    const trigger = extractVoiceTrigger(cleanText, wakePhraseList(wakePhrases))
    if (!trigger) {
      return appendTranscriptSegment(cleanText, confidence, speakerLabel, source)
    }

    let nextSegments = segmentsRef.current
    if (trigger.beforeText) {
      nextSegments = appendTranscriptSegment(trigger.beforeText, confidence, speakerLabel, source)
    }

    startWakeCapture(trigger)
    if (trigger.question) {
      appendWakeCommandText(trigger.question)
    }
    if (options.finishWakeImmediately) {
      finalizeWakeCapture()
    }

    return nextSegments
  }

  async function startStreamingAsr() {
    if (asrRecordingRef.current || speechStatus === 'connecting' || speechStatus === 'finalizing') {
      return
    }
    if (!hasAsrProvider(asrProvider) || !hasTextProvider(provider)) {
      setCaptureLog((current) => [
        {
          ok: false,
          label: 'Provider configuration required',
          detail: '请先完整配置云端 ASR Provider 和 Agent Provider。',
        },
        ...current,
      ].slice(0, 4))
      return
    }

    const meetingId = activeMeetingIdRef.current
    const recordingRunId = makeId('run')
    const startToken = streamingStartTokenRef.current + 1
    streamingStartTokenRef.current = startToken
    let startedSessionId: string | null = null
    pcmBufferRef.current = []
    pcmSampleCountRef.current = 0
    setSpeechStatus('connecting')
    setMode('recording')
    setInterimTranscript('正在连接 DashScope 实时 ASR...')

    try {
      const session = await startStreamingAsrSession({
        provider: asrProvider,
        language: speechLang,
        meetingId,
        recordingRunId,
      })
      startedSessionId = session.sessionId
      if (streamingStartTokenRef.current !== startToken) {
        await cancelStreamingAsrSession(session.sessionId)
        return
      }
      const sessionInfo: StreamingAsrSessionInfo = {
        sessionId: session.sessionId,
        meetingId,
        recordingRunId,
        providerLabel: session.providerLabel,
        startedAt: performance.now(),
        pendingFrames: 0,
        droppedFrames: 0,
        lastLatencyMs: 0,
      }
      streamingSessionsRef.current.set(session.sessionId, sessionInfo)
      currentStreamingSessionIdRef.current = session.sessionId
      incrementAsrPending(meetingId)

      const stream = await navigator.mediaDevices.getUserMedia({ audio: true })
      if (streamingStartTokenRef.current !== startToken) {
        stream.getTracks().forEach((track) => track.stop())
        streamingSessionsRef.current.delete(session.sessionId)
        streamingAudioPushChainsRef.current.delete(session.sessionId)
        currentStreamingSessionIdRef.current = null
        decrementAsrPending(meetingId)
        await cancelStreamingAsrSession(session.sessionId)
        return
      }
      const AudioContextCtor = window.AudioContext || (window as WindowWithAudioContext).webkitAudioContext
      if (!AudioContextCtor) {
        throw new Error('当前 WebView/浏览器不支持 AudioContext。')
      }
      const audioContext = new AudioContextCtor()
      const source = audioContext.createMediaStreamSource(stream)
      const processor = audioContext.createScriptProcessor(4096, 1, 1)
      const frameSourceSamples = Math.max(1, Math.round((audioContext.sampleRate * streamingFrameMs) / 1000))

      processor.onaudioprocess = (event) => {
        const output = event.outputBuffer.getChannelData(0)
        output.fill(0)

        if (!asrRecordingRef.current) {
          return
        }

        const input = event.inputBuffer.getChannelData(0)
        const copy = new Float32Array(input.length)
        copy.set(input)
        pcmBufferRef.current.push(copy)
        pcmSampleCountRef.current += copy.length

        while (pcmSampleCountRef.current >= frameSourceSamples) {
          const frame = consumeSamples(frameSourceSamples)
          queueStreamingAudioFrame(session.sessionId, frame, audioContext.sampleRate)
        }
      }

      source.connect(processor)
      processor.connect(audioContext.destination)
      mediaStreamRef.current = stream
      audioContextRef.current = audioContext
      audioSourceRef.current = source
      audioProcessorRef.current = processor
      asrRecordingRef.current = true
      setSpeechStatus('listening')
      setInterimTranscript('实时 ASR 已连接，正在等待语音。')
      syncStreamingAsrActivity()

      setCaptureLog((current) => [
        {
          ok: true,
          label: 'Streaming ASR started',
          detail: `${session.providerLabel} 已连接。音频格式：16kHz mono PCM16，${streamingFrameMs} ms/frame。`,
        },
        ...current,
      ].slice(0, 4))
    } catch (error) {
      asrRecordingRef.current = false
      stopAudioGraph()
      if (startedSessionId) {
        currentStreamingSessionIdRef.current = null
        streamingSessionsRef.current.delete(startedSessionId)
        streamingAudioPushChainsRef.current.delete(startedSessionId)
        decrementAsrPending(meetingId)
        void cancelStreamingAsrSession(startedSessionId)
      }
      setSpeechStatus('error')
      setInterimTranscript('')
      setCaptureLog((current) => [
        {
          ok: false,
          label: 'Streaming ASR start failed',
          detail: error instanceof Error ? error.message : '无法启动实时 ASR。',
        },
        ...current,
      ].slice(0, 4))
    }
  }

  function stopStreamingAsr(
    meetingId = activeMeetingIdRef.current,
    options: { flushFinal?: boolean } = { flushFinal: true },
  ) {
    streamingStartTokenRef.current += 1
    const sessionId = currentStreamingSessionIdRef.current
    const session = sessionId ? streamingSessionsRef.current.get(sessionId) : null
    const sampleRate = audioContextRef.current?.sampleRate ?? streamingSampleRate
    const targetMeetingId = session?.meetingId ?? meetingId
    asrRecordingRef.current = false
    currentStreamingSessionIdRef.current = null

    if (session && options.flushFinal !== false) {
      flushStreamingAudioRemainder(session.sessionId, sampleRate)
    }

    stopAudioGraph()

    if (!session) {
      setSpeechStatus('idle')
      if ((asrPendingCountRef.current.get(targetMeetingId) ?? 0) === 0) {
        setInterimTranscript('')
      }
      syncStreamingAsrActivity()
      return
    }

    if (options.flushFinal === false) {
      streamingSessionsRef.current.delete(session.sessionId)
      streamingAudioPushChainsRef.current.delete(session.sessionId)
      decrementAsrPending(session.meetingId)
      void cancelStreamingAsrSession(session.sessionId)
      setSpeechStatus('idle')
      setInterimTranscript('')
      syncStreamingAsrActivity()
      return
    }

    if (session.meetingId === activeMeetingIdRef.current) {
      setSpeechStatus('finalizing')
      setInterimTranscript('正在结束实时 ASR，等待 final 结果。')
    }
    void finishStreamingSessionAfterQueuedAudio(session.sessionId).catch((error) => {
      handleStreamingAsrFailure(session.sessionId, error instanceof Error ? error.message : '实时 ASR finish-task 发送失败。')
    })
  }

  function waitForAsrIdle(meetingId: string): Promise<void> {
    if ((asrPendingCountRef.current.get(meetingId) ?? 0) === 0) {
      return Promise.resolve()
    }

    return new Promise((resolve) => {
      const resolvers = asrIdleResolversRef.current.get(meetingId) ?? []
      resolvers.push(resolve)
      asrIdleResolversRef.current.set(meetingId, resolvers)
    })
  }

  function resolveAsrIdleIfDone(meetingId: string) {
    if ((asrPendingCountRef.current.get(meetingId) ?? 0) > 0) {
      return
    }

    const resolvers = asrIdleResolversRef.current.get(meetingId) ?? []
    asrIdleResolversRef.current.delete(meetingId)
    for (const resolve of resolvers) {
      resolve()
    }
  }

  function incrementAsrPending(meetingId: string) {
    asrPendingCountRef.current.set(meetingId, (asrPendingCountRef.current.get(meetingId) ?? 0) + 1)
    syncStreamingAsrActivity()
  }

  function decrementAsrPending(meetingId: string) {
    const nextCount = Math.max(0, (asrPendingCountRef.current.get(meetingId) ?? 0) - 1)
    if (nextCount === 0) {
      asrPendingCountRef.current.delete(meetingId)
    } else {
      asrPendingCountRef.current.set(meetingId, nextCount)
    }
    resolveAsrIdleIfDone(meetingId)
    syncStreamingAsrActivity()
  }

  function syncStreamingAsrActivity() {
    const activeMeetingId = activeMeetingIdRef.current
    const activeSessions = Array.from(streamingSessionsRef.current.values()).filter(
      (session) => session.meetingId === activeMeetingId,
    )
    setAsrActivity((current) => ({
      queued: activeSessions.reduce((sum, session) => sum + session.pendingFrames, 0),
      inFlight: activeSessions.length,
      lastLatencyMs: activeSessions.at(-1)?.lastLatencyMs ?? current.lastLatencyMs,
    }))
  }

  function consumeSamples(sampleCount: number): Float32Array {
    const targetCount = Math.min(sampleCount, pcmSampleCountRef.current)
    const frame = new Float32Array(targetCount)
    const nextChunks: Float32Array[] = []
    let copied = 0
    let remainingToCopy = targetCount

    for (const chunk of pcmBufferRef.current) {
      if (remainingToCopy <= 0) {
        nextChunks.push(chunk)
        continue
      }

      const take = Math.min(remainingToCopy, chunk.length)
      frame.set(chunk.subarray(0, take), copied)
      copied += take
      remainingToCopy -= take

      if (take < chunk.length) {
        nextChunks.push(chunk.subarray(take))
      }
    }

    pcmBufferRef.current = nextChunks
    pcmSampleCountRef.current = Math.max(0, pcmSampleCountRef.current - targetCount)
    return frame
  }

  function queueStreamingAudioFrame(sessionId: string, samples: Float32Array, inputSampleRate: number) {
    const session = streamingSessionsRef.current.get(sessionId)
    if (!session) {
      return
    }

    const audioBase64 = pcm16ToBase64(resampleLinear(samples, inputSampleRate, streamingSampleRate))
    session.pendingFrames += 1
    syncStreamingAsrActivity()
    const previous = streamingAudioPushChainsRef.current.get(sessionId) ?? Promise.resolve()
    const next = previous
      .catch(() => undefined)
      .then(() => pushStreamingAsrAudio({ sessionId, audioBase64 }))
      .catch((error) => {
        const current = streamingSessionsRef.current.get(sessionId)
        if (current && !current.pushErrorReported) {
          current.pushErrorReported = true
          setCaptureLog((logs) => [
            {
              ok: false,
              label: 'Streaming ASR audio failed',
              detail: error instanceof Error ? error.message : '实时 ASR 音频帧发送失败。',
            },
            ...logs,
          ].slice(0, 4))
        }
      })
      .finally(() => {
        const current = streamingSessionsRef.current.get(sessionId)
        if (current) {
          current.pendingFrames = Math.max(0, current.pendingFrames - 1)
          syncStreamingAsrActivity()
        }
      })

    streamingAudioPushChainsRef.current.set(sessionId, next)
  }

  function flushStreamingAudioRemainder(sessionId: string, sampleRate: number) {
    const minSamples = Math.max(1, Math.round((sampleRate * streamingRemainderMinMs) / 1000))
    if (pcmSampleCountRef.current < minSamples) {
      return
    }

    const frame = consumeSamples(pcmSampleCountRef.current)
    queueStreamingAudioFrame(sessionId, frame, sampleRate)
  }

  async function finishStreamingSessionAfterQueuedAudio(sessionId: string) {
    await (streamingAudioPushChainsRef.current.get(sessionId) ?? Promise.resolve())
    await finishStreamingAsrSession(sessionId)
  }

  function cancelCurrentStreamingAsr() {
    streamingStartTokenRef.current += 1
    const sessionId = currentStreamingSessionIdRef.current
    currentStreamingSessionIdRef.current = null
    asrRecordingRef.current = false
    stopAudioGraph()
    if (!sessionId) {
      return
    }
    const session = streamingSessionsRef.current.get(sessionId)
    if (session) {
      streamingSessionsRef.current.delete(sessionId)
      streamingAudioPushChainsRef.current.delete(sessionId)
      decrementAsrPending(session.meetingId)
    }
    void cancelStreamingAsrSession(sessionId)
  }

  function handleStreamingAsrEvent(event: StreamingAsrEvent) {
    const session = streamingSessionsRef.current.get(event.sessionId)
    if (!session) {
      return
    }

    if (event.status === 'interim') {
      if (event.meetingId === activeMeetingIdRef.current) {
        if (!session.firstInterimAt) {
          session.firstInterimAt = performance.now()
          session.lastLatencyMs = Math.round(session.firstInterimAt - session.startedAt)
          syncStreamingAsrActivity()
        }
        setInterimTranscript(event.text)
      }
      return
    }

    if (event.status === 'final') {
      if (!session.firstFinalAt) {
        session.firstFinalAt = performance.now()
        session.lastLatencyMs = Math.round(session.firstFinalAt - session.startedAt)
        syncStreamingAsrActivity()
      }
      const text = event.text.trim()
      if (text) {
        if (event.meetingId === activeMeetingIdRef.current) {
          handleRecognizedTranscript(text, 0.9, 'Me', 'microphone')
        } else {
          appendHistoricalTranscriptSegment(event.meetingId, text, 0.9, 'Me', 'microphone')
        }
      }
      return
    }

    if (event.status === 'error') {
      handleStreamingAsrFailure(event.sessionId, event.errorMessage || '实时 ASR 连接失败。')
      return
    }

    if (event.status === 'finished') {
      finishStreamingAsrLocally(event.sessionId)
    }
  }

  function handleStreamingAsrFailure(sessionId: string, message: string) {
    const session = streamingSessionsRef.current.get(sessionId)
    if (!session) {
      return
    }

    if (session.meetingId === activeMeetingIdRef.current) {
      setSpeechStatus('error')
      setInterimTranscript('')
    } else {
      updateHistoricalMeeting(session.meetingId, (meeting) => ({
        ...meeting,
        status: 'error',
        error: message,
        updatedAt: new Date().toISOString(),
      }))
    }

    setCaptureLog((current) => [
      {
        ok: false,
        label: 'Streaming ASR failed',
        detail: message,
      },
      ...current,
    ].slice(0, 4))
    finishStreamingAsrLocally(sessionId)
  }

  function finishStreamingAsrLocally(sessionId: string) {
    const session = streamingSessionsRef.current.get(sessionId)
    if (!session) {
      return
    }

    streamingSessionsRef.current.delete(sessionId)
    streamingAudioPushChainsRef.current.delete(sessionId)
    decrementAsrPending(session.meetingId)

    if (currentStreamingSessionIdRef.current === sessionId) {
      currentStreamingSessionIdRef.current = null
    }
    if (session.meetingId === activeMeetingIdRef.current) {
      setSpeechStatus('idle')
      setInterimTranscript('')
      setMode((current) => (current === 'recording' ? 'paused' : current))
    }
    setCaptureLog((current) => [
      {
        ok: true,
        label: 'Streaming ASR finished',
        detail: `${session.providerLabel} 已完成本次 Recording Run。`,
      },
      ...current,
    ].slice(0, 4))
  }

  function stopAudioGraph() {
    audioProcessorRef.current?.disconnect()
    audioSourceRef.current?.disconnect()
    mediaStreamRef.current?.getTracks().forEach((track) => track.stop())
    void audioContextRef.current?.close()
    audioProcessorRef.current = null
    audioSourceRef.current = null
    mediaStreamRef.current = null
    audioContextRef.current = null
    pcmBufferRef.current = []
    pcmSampleCountRef.current = 0
  }

  function stopActiveTranscription() {
    const meetingId = activeMeetingIdRef.current
    if (asrRecordingRef.current || currentStreamingSessionIdRef.current || speechStatus === 'connecting') {
      stopStreamingAsr(meetingId)
    } else {
      setSpeechStatus('idle')
    }

    cancelWakeCapture()
    if (segmentsRef.current.length > 0 || responsesRef.current.length > 0 || meetingDigestRef.current.text) {
      archiveActiveMeeting(meetingId)
    }
    resetActiveMeeting('paused')
  }

  function appendHistoricalTranscriptSegment(
    meetingId: string,
    text: string,
    confidence: number,
    speakerLabel: string,
    source: AudioSource,
  ) {
    const cleanText = normalizeTranscriptText(text)
    if (!cleanText) {
      return
    }

    const trigger = extractVoiceTrigger(cleanText, wakePhraseList(wakePhrases))
    const meetingText = trigger ? trigger.beforeText : cleanText
    if (!meetingText) {
      return
    }

    updateHistoricalMeeting(meetingId, (meeting) => {
      const lastEnd = meeting.segments.at(-1)?.endMs ?? 0
      const estimatedMs = Math.max(1400, Math.min(15000, meetingText.length * 190))
      const segment: MeetingSegment = {
        id: makeId('seg'),
        speakerLabel,
        source,
        startMs: lastEnd + 350,
        endMs: lastEnd + 350 + estimatedMs,
        text: meetingText,
        confidence,
        status: 'final',
        createdAt: new Date().toISOString(),
      }
      const nextSegments = [...meeting.segments, segment]

      return {
        ...meeting,
        segments: nextSegments,
        title: makeMeetingTitle(meeting.digest, nextSegments, meeting.createdAt),
        updatedAt: new Date().toISOString(),
      }
    })
  }

  function appendTranscriptSegment(
    text: string,
    confidence: number,
    speakerLabel: string,
    source: AudioSource,
  ): MeetingSegment[] {
    const currentSegments = segmentsRef.current
    const lastEnd = currentSegments.at(-1)?.endMs ?? 0
    const estimatedMs = Math.max(1400, Math.min(15000, text.length * 190))
    const segment: MeetingSegment = {
      id: makeId('seg'),
      speakerLabel,
      source,
      startMs: lastEnd + 350,
      endMs: lastEnd + 350 + estimatedMs,
      text,
      confidence,
      status: 'final',
      createdAt: new Date().toISOString(),
    }
    const nextSegments = [...currentSegments, segment]

    segmentsRef.current = nextSegments
    setSegments(nextSegments)
    setInterimTranscript('')
    setMode((current) => (current === 'paused' ? current : 'recording'))
    return nextSegments
  }

  const digestCopy = meetingDigest.text
    ? meetingDigest.text
    : segments.length === 0
      ? '还没有会议上下文。'
      : hasTextProvider(provider)
        ? '正在等待下一次递增纪要更新。'
        : '已记录原始转写。配置 Agent Provider 的文字模型后，将在这里生成递增会议纪要。'
  const pendingDigestSegments = Math.max(0, segments.length - meetingDigest.segmentCount)
  const pendingAsrChunks = asrActivity.queued + asrActivity.inFlight
  const digestMeta = digestStatusLabel(digestStatus, meetingDigest, pendingDigestSegments, pendingAsrChunks)
  const activeMeetingTitle = makeMeetingTitle(meetingDigest, segments, activeMeetingCreatedAt)
  const configIssues = providerConfigurationIssues()
  const providersReady = configIssues.length === 0
  const providerSummary = hasTextProvider(provider) ? `Agent ${provider.model}` : 'Agent not configured'
  const asrSummary = hasAsrProvider(asrProvider) ? `Cloud ASR ${asrProvider.model}` : 'ASR not configured'
  const asrActivitySummary =
    asrActivity.queued > 0 || asrActivity.inFlight > 0
      ? `Streaming sessions ${asrActivity.inFlight}, pending frames ${asrActivity.queued}.`
      : asrActivity.lastLatencyMs > 0
        ? `First ASR ${asrActivity.lastLatencyMs} ms.`
        : 'ASR ready.'
  const searchSummary = `Search ${searchConfig.mode}`

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand-row">
          <div className="brand-mark">
            <Radio size={22} />
          </div>
          <div>
            <p className="eyebrow">AIMeeting</p>
            <h1>Meeting Copilot</h1>
          </div>
        </div>

        <section className="settings-panel history-panel">
          <div className="panel-title">
            <Clock size={16} />
            <span>Meeting history</span>
          </div>
          <div className="history-toolbar">
            <span>{meetings.length} sessions</span>
            <button type="button" onClick={toggleAllHistoryChecked} disabled={meetings.length === 0}>
              {checkedHistoryIds.length === meetings.length && meetings.length > 0 ? 'Unselect' : 'Select all'}
            </button>
          </div>
          {meetings.length === 0 ? (
            <p className="muted-copy">Stop 后会在这里保存上一段会议。</p>
          ) : (
            <div className="history-list">
              {meetings.map((meeting) => (
                <div
                  key={meeting.id}
                  className={`history-item ${selectedHistoryId === meeting.id ? 'selected' : ''}`}
                >
                  <input
                    type="checkbox"
                    checked={checkedHistoryIdSet.has(meeting.id)}
                    onChange={() => toggleHistoryChecked(meeting.id)}
                    aria-label={`Select ${meeting.title}`}
                  />
                  <button
                    type="button"
                    className="history-open"
                    onClick={() => setSelectedHistoryId((current) => (current === meeting.id ? null : meeting.id))}
                  >
                    <span>{meeting.title}</span>
                    <small>
                      {meeting.status} · {formatClock(meeting.stoppedAt)}
                    </small>
                  </button>
                </div>
              ))}
            </div>
          )}
          <div className="history-cleanup">
            <button
              type="button"
              className="icon-command history-detail-button"
              onClick={() => setShowHistoryDetails(true)}
              disabled={!selectedHistory}
            >
              <FileText size={15} />
              <span>Details</span>
            </button>
            <button type="button" className="icon-command" onClick={deleteCheckedHistory} disabled={checkedHistoryIds.length === 0}>
              <Trash2 size={15} />
              <span>Delete selected</span>
            </button>
            <button type="button" className="icon-command" onClick={deleteAllHistory} disabled={meetings.length === 0}>
              <Trash2 size={15} />
              <span>Clear all</span>
            </button>
          </div>
          {selectedHistory ? (
            <div className="history-preview">
              <strong>{selectedHistory.title}</strong>
              <p>{selectedHistory.digest.text || '这段会议还没有生成纪要。'}</p>
              <small>
                {selectedHistory.segments.length} segments · {selectedHistory.status}
              </small>
            </div>
          ) : null}
        </section>

        <section className="settings-panel voice-panel">
          <div className="panel-title">
            <Mic size={16} />
            <span>Mic transcription</span>
          </div>
          <div className={`mode-pill compact ${modeMeta[mode].tone}`}>
            <Activity size={15} />
            <span>{modeMeta[mode].label}</span>
          </div>
          <div className={`speech-state ${speechStatus}`}>
            <span>{speechStatusLabel(speechStatus)}</span>
            <small>
              {providersReady
                ? `Cloud ASR ${asrProvider.model}，${asrActivitySummary}`
                : `缺少配置：${configIssues.join('、')}`}
            </small>
          </div>
          <div className="voice-actions">
            <button
              type="button"
              className="icon-command primary"
              onClick={startStreamingAsr}
              disabled={speechStatus === 'connecting' || speechStatus === 'listening' || speechStatus === 'finalizing' || !providersReady}
            >
              <Mic size={18} />
              <span>{mode === 'paused' && (segments.length > 0 || meetingDigest.text) ? 'Resume mic' : 'Start mic'}</span>
            </button>
            <button
              type="button"
              className="icon-command"
              onClick={pauseActiveMeeting}
              disabled={speechStatus !== 'listening'}
            >
              <Pause size={18} />
              <span>Pause</span>
            </button>
            <button
              type="button"
              className="icon-command"
              onClick={stopActiveTranscription}
              disabled={!['connecting', 'listening', 'finalizing'].includes(speechStatus) && segments.length === 0 && !meetingDigest.text}
            >
              <Square size={17} />
              <span>Stop</span>
            </button>
          </div>
          <details className="voice-options">
            <summary>
              <span>Voice options</span>
              <small>{speechLang}</small>
            </summary>
            <label>
              <span>Language</span>
              <select value={speechLang} onChange={(event) => setSpeechLang(event.target.value)}>
                <option value="zh-CN">普通话 zh-CN</option>
                <option value="zh-HK">粤语/香港 zh-HK</option>
                <option value="en-US">English en-US</option>
                <option value="ja-JP">日本語 ja-JP</option>
              </select>
            </label>
            <label>
              <span>Wake phrases</span>
              <input
                value={wakePhrases}
                onChange={(event) => setWakePhrases(event.target.value)}
                placeholder="嗨助手,嘿助手,hey assistant"
              />
            </label>
            <label className="check-line">
              <input
                type="checkbox"
                checked={autoAskOnWake}
                onChange={(event) => setAutoAskOnWake(event.target.checked)}
              />
              <span>Auto ask after wake</span>
            </label>
          </details>
          <button type="button" className="icon-command" onClick={clearMeeting} title="Clear current meeting notes">
            <Trash2 size={17} />
            <span>Clear current</span>
          </button>
        </section>

        <section className="settings-panel advanced-settings">
          <button
            type="button"
            className="advanced-trigger"
            onClick={() => setShowAdvancedSettings(true)}
            aria-haspopup="dialog"
          >
            <div className="panel-title">
              <Settings size={16} />
              <span>Advanced settings</span>
            </div>
            <div className="settings-summary">
              <span>{asrSummary}</span>
              <span>{providerSummary}</span>
              <span>{searchSummary}</span>
            </div>
          </button>
        </section>
      </aside>

      <main className="workspace">
        <header className="topbar">
          <div>
            <p className="eyebrow">Current meeting</p>
            <h2>{activeMeetingTitle}</h2>
          </div>
          <div className="capture-actions">
            <button type="button" className="ghost-button" onClick={() => probe('mic')}>
              <Mic size={17} />
              <span>Mic probe</span>
            </button>
            <button type="button" className="ghost-button" onClick={() => probe('system')}>
              <Monitor size={17} />
              <span>System probe</span>
            </button>
          </div>
        </header>

        {!providersReady ? (
          <section className="config-alert">
            <div>
              <strong>需要先完成云端配置</strong>
              <p>请填写 {configIssues.join('、')} 的 Base URL、Model 和 API key。API key 只在本次运行中使用，重启后需要重新填写。</p>
            </div>
            <button type="button" className="icon-command primary" onClick={() => setShowAdvancedSettings(true)}>
              <Settings size={17} />
              <span>Open settings</span>
            </button>
          </section>
        ) : null}

        <section className="metrics">
          <Metric icon={Clock} label="Duration" value={formatDuration(durationMs)} />
          <Metric icon={FileText} label="Raw segments" value={segments.length.toString()} />
          <Metric icon={Database} label="Memory" value={`${Math.max(1, Math.round(segments.length * 1.8))} KB`} />
          <Metric icon={Shield} label="Audio retention" value="Transient" />
        </section>

        <section className="memory-band">
          <div className="panel-title">
            <Sparkles size={17} />
            <span>Meeting Digest</span>
          </div>
          <p className="digest-text">{digestCopy}</p>
          <div className="digest-meta">
            <span>{digestMeta}</span>
            {meetingDigest.providerLabel ? <span>{meetingDigest.providerLabel}</span> : null}
          </div>
          {interimTranscript ? (
            <div className="interim-line">
              <Languages size={15} />
              <span>{interimTranscript}</span>
            </div>
          ) : null}
          {lastVoiceTrigger ? (
            <div className="wake-line">
              <Keyboard size={15} />
              <span>
                已捕获触发词“{lastVoiceTrigger.phrase}”
                {lastVoiceTrigger.question ? `，助手请求：${lastVoiceTrigger.question}` : '，正在采集助手请求。'}
              </span>
            </div>
          ) : null}
          <div className="source-strip">
            {sourceCounts.map((item) => (
              <span key={item.source}>
                {item.source}: {item.count}
              </span>
            ))}
          </div>
        </section>

        <section className="timeline-section raw-debug-section">
          <details>
            <summary className="section-head">
              <div>
                <p className="eyebrow">Debug transcript</p>
                <h2>Raw ASR segments</h2>
              </div>
              <span className="small-badge">
                {segments.filter((segment) => segment.status === 'final').length} final
              </span>
            </summary>
            <div className="timeline-list">
              {segments.length === 0 ? (
                <div className="empty-timeline">
                  <Mic size={24} />
                  <p>完成云端配置后点击 Start mic 开始记录，或手动添加一段测试文本。</p>
                </div>
              ) : (
                segments
                  .slice()
                  .reverse()
                  .map((segment) => <SegmentRow key={segment.id} segment={segment} />)
              )}
            </div>
          </details>
        </section>

        <section className="manual-entry">
          <details>
            <summary className="section-head compact">
              <div className="panel-title">
                <Plus size={16} />
                <span>Add transcript segment</span>
              </div>
              <span className="small-badge">test tool</span>
            </summary>
            <form onSubmit={addManualSegment}>
              <div className="entry-grid">
                <select value={manualSpeaker} onChange={(event) => setManualSpeaker(event.target.value)}>
                  {speakerOptions.map((speaker) => (
                    <option key={speaker} value={speaker}>
                      {speaker}
                    </option>
                  ))}
                </select>
                <select value={manualSource} onChange={(event) => setManualSource(event.target.value as AudioSource)}>
                  {sourceOptions.map((source) => (
                    <option key={source} value={source}>
                      {source}
                    </option>
                  ))}
                </select>
                <input
                  value={manualText}
                  onChange={(event) => setManualText(event.target.value)}
                  placeholder="输入测试文本，例如：嗨助手 总结刚才内容"
                />
                <button type="submit" className="icon-command primary">
                  <Plus size={17} />
                  <span>Add</span>
                </button>
              </div>
            </form>
          </details>
        </section>
      </main>

      <aside className="copilot-panel">
        <div className="panel-title large">
          <Bot size={19} />
          <span>Copilot</span>
        </div>
        <form className="ask-box" onSubmit={askAgent}>
          <textarea
            value={question}
            onChange={(event) => setQuestion(event.target.value)}
            rows={5}
            placeholder={hasTextProvider(provider) ? '输入问题，或说：嗨助手 总结刚才内容' : '先配置 Agent Provider 后再提问'}
          />
          <button type="submit" className="send-button" disabled={isThinking || !question.trim() || !hasTextProvider(provider)}>
            {isThinking ? <Loader2 size={18} className="spin" /> : <Send size={18} />}
            <span>{isThinking ? 'Thinking' : 'Ask'}</span>
          </button>
        </form>

        {latestResponse ? (
          <article className="answer-card">
            <div className="answer-meta">
              <span>{formatClock(latestResponse.createdAt)}</span>
              <span>{latestResponse.latencyMs} ms</span>
              <span>{latestResponse.providerLabel}</span>
            </div>
            <h3>{latestResponse.question}</h3>
            <p className="answer-text">{latestResponse.answer}</p>
            {latestResponse.error ? <p className="error-line">{latestResponse.error}</p> : null}

            <div className="plan-list">
              <div className="panel-title">
                <ListChecks size={16} />
                <span>Plan</span>
              </div>
              {latestResponse.planItems.map((item) => (
                <p key={item}>{item}</p>
              ))}
            </div>

            <button type="button" className="evidence-toggle" onClick={() => setShowEvidence((current) => !current)}>
              <Eye size={16} />
              <span>{showEvidence ? 'Hide evidence' : 'Show evidence'}</span>
            </button>

            {showEvidence ? <EvidenceList response={latestResponse} /> : null}
          </article>
        ) : (
          <div className="empty-state">
            <Bot size={24} />
            <p>等待会议内容后再提问。</p>
          </div>
        )}

        <section className="search-log">
          <div className="panel-title">
            <Globe2 size={16} />
            <span>Search trail</span>
          </div>
          <SearchTrail traces={latestResponse?.searches ?? []} />
        </section>

        <section className="capture-log">
          <div className="panel-title">
            <Activity size={16} />
            <span>Capture log</span>
          </div>
          {captureLog.length === 0 ? (
            <p className="muted-copy">暂无事件。</p>
          ) : (
            captureLog.map((probeResult) => (
              <div key={`${probeResult.label}-${probeResult.detail}`} className={`probe-row ${probeResult.ok ? 'ok' : 'bad'}`}>
                <span>{probeResult.label}</span>
                <p>{probeResult.detail}</p>
              </div>
            ))
          )}
        </section>
      </aside>

      {showAdvancedSettings ? (
        <div className="modal-backdrop" onMouseDown={() => setShowAdvancedSettings(false)}>
          <section
            className="settings-modal"
            role="dialog"
            aria-modal="true"
            aria-labelledby="advanced-settings-title"
            onMouseDown={(event) => event.stopPropagation()}
          >
            <header className="settings-modal-head">
              <div>
                <p className="eyebrow">Configuration</p>
                <h2 id="advanced-settings-title">Advanced settings</h2>
              </div>
              <button
                type="button"
                className="modal-close"
                onClick={() => setShowAdvancedSettings(false)}
                aria-label="Close advanced settings"
              >
                <X size={18} />
              </button>
            </header>
            <div className="settings-stack">
              <div className="settings-group">
                <div className="panel-title">
                  <Languages size={16} />
                  <span>ASR Provider</span>
                </div>
                <label>
                  <span>Base URL</span>
                  <input
                    value={asrProvider.baseUrl}
                    onChange={(event) => setAsrProvider((current) => ({ ...current, baseUrl: event.target.value }))}
                    placeholder="https://dashscope.aliyuncs.com/compatible-mode/v1"
                  />
                </label>
                <label>
                  <span>Model</span>
                  <input
                    value={asrProvider.model}
                    onChange={(event) => setAsrProvider((current) => ({ ...current, model: event.target.value }))}
                    placeholder="paraformer-realtime-v2"
                  />
                </label>
                <label>
                  <span>API key</span>
                  <input
                    type="password"
                    value={asrProvider.apiKey}
                    onChange={(event) => setAsrProvider((current) => ({ ...current, apiKey: event.target.value }))}
                    placeholder="kept in memory"
                  />
                </label>
              </div>

              <div className="settings-group">
                <div className="panel-title">
                  <Settings size={16} />
                  <span>Agent Provider</span>
                </div>
                <label>
                  <span>Base URL</span>
                  <input
                    value={provider.baseUrl}
                    onChange={(event) => setProvider((current) => ({ ...current, baseUrl: event.target.value }))}
                    placeholder="https://api.openai.com/v1"
                  />
                </label>
                <label>
                  <span>Model</span>
                  <input
                    value={provider.model}
                    onChange={(event) => setProvider((current) => ({ ...current, model: event.target.value }))}
                    placeholder="gpt-4.1-mini"
                  />
                </label>
                <label>
                  <span>API key</span>
                  <input
                    type="password"
                    value={provider.apiKey}
                    onChange={(event) => setProvider((current) => ({ ...current, apiKey: event.target.value }))}
                    placeholder="kept in memory"
                  />
                </label>
                <div className="two-column">
                  <label>
                    <span>Endpoint</span>
                    <select
                      value={provider.endpointFlavor}
                      onChange={(event) =>
                        setProvider((current) => ({
                          ...current,
                          endpointFlavor: event.target.value as ProviderConfig['endpointFlavor'],
                        }))
                      }
                    >
                      <option value="chat-completions">chat</option>
                      <option value="responses">responses</option>
                    </select>
                  </label>
                  <label>
                    <span>Temp</span>
                    <input
                      type="number"
                      min="0"
                      max="1"
                      step="0.05"
                      value={provider.temperature}
                      onChange={(event) =>
                        setProvider((current) => ({ ...current, temperature: Number(event.target.value) }))
                      }
                    />
                  </label>
                </div>
              </div>

              <div className="settings-group">
                <div className="panel-title">
                  <Search size={16} />
                  <span>Search</span>
                </div>
                <label>
                  <span>Mode</span>
                  <select
                    value={searchConfig.mode}
                    onChange={(event) =>
                      setSearchConfig((current) => ({ ...current, mode: event.target.value as SearchConfig['mode'] }))
                    }
                  >
                    <option value="auto">auto</option>
                    <option value="confirm">confirm</option>
                    <option value="off">off</option>
                  </select>
                </label>
                <label>
                  <span>Endpoint template</span>
                  <input
                    value={searchConfig.endpointTemplate}
                    onChange={(event) =>
                      setSearchConfig((current) => ({ ...current, endpointTemplate: event.target.value }))
                    }
                    placeholder="https://search/api?q={query}"
                  />
                </label>
                <label className="check-line">
                  <input
                    type="checkbox"
                    checked={searchConfig.redactBeforeSearch}
                    onChange={(event) =>
                      setSearchConfig((current) => ({ ...current, redactBeforeSearch: event.target.checked }))
                    }
                  />
                  <span>Redact query</span>
                </label>
              </div>
            </div>
            <footer className="settings-modal-foot">
              <button type="button" className="icon-command primary" onClick={() => setShowAdvancedSettings(false)}>
                <span>确定</span>
              </button>
            </footer>
          </section>
        </div>
      ) : null}

      {showHistoryDetails && selectedHistory ? (
        <div className="modal-backdrop" onMouseDown={() => setShowHistoryDetails(false)}>
          <section
            className="settings-modal history-detail-modal"
            role="dialog"
            aria-modal="true"
            aria-labelledby="history-detail-title"
            onMouseDown={(event) => event.stopPropagation()}
          >
            <header className="settings-modal-head">
              <div>
                <p className="eyebrow">Archived Meeting</p>
                <h2 id="history-detail-title">{selectedHistory.title}</h2>
              </div>
              <button
                type="button"
                className="modal-close"
                onClick={() => setShowHistoryDetails(false)}
                aria-label="Close session details"
              >
                <X size={18} />
              </button>
            </header>

            <div className="history-detail-content">
              <div className="history-detail-meta">
                <span>{selectedHistory.status}</span>
                <span>{selectedHistory.segments.length} segments</span>
                <span>Stopped {formatClock(selectedHistory.stoppedAt)}</span>
                {selectedHistory.digest.providerLabel ? <span>{selectedHistory.digest.providerLabel}</span> : null}
              </div>
              {selectedHistory.status === 'finalizing' ? (
                <p className="history-detail-notice">这段会议还在整理中，稍后会更新最终纪要。</p>
              ) : null}
              {selectedHistory.error ? <p className="error-line">{selectedHistory.error}</p> : null}
              <article className="history-detail-digest">
                {selectedHistory.digest.text || '这段会议还没有生成纪要。'}
              </article>
              <small>
                Updated{' '}
                {selectedHistory.digest.updatedAt
                  ? new Date(selectedHistory.digest.updatedAt).toLocaleString()
                  : new Date(selectedHistory.updatedAt).toLocaleString()}
              </small>
            </div>

            <footer className="settings-modal-foot">
              <button type="button" className="icon-command primary" onClick={() => setShowHistoryDetails(false)}>
                <span>确定</span>
              </button>
            </footer>
          </section>
        </div>
      ) : null}
    </div>
  )
}

function Metric({ icon: Icon, label, value }: { icon: LucideIcon; label: string; value: string }) {
  return (
    <div className="metric">
      <Icon size={18} />
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  )
}

function SegmentRow({ segment }: { segment: MeetingSegment }) {
  return (
    <article className="segment-row">
      <div className="segment-time">
        <span>{formatDuration(segment.startMs)}</span>
        <small>{segment.source}</small>
      </div>
      <div className="segment-body">
        <div>
          <strong>{segment.speakerLabel}</strong>
          <span>{Math.round(segment.confidence * 100)}%</span>
        </div>
        <p>{segment.text}</p>
      </div>
    </article>
  )
}

function EvidenceList({ response }: { response: AgentResponse }) {
  return (
    <div className="evidence-list">
      {response.evidence.map((item) => (
        <div key={item.id} className="evidence-item">
          <span>{item.kind}</span>
          <strong>{item.title}</strong>
          <p>{item.detail}</p>
          {item.url ? (
            <a href={item.url} target="_blank" rel="noreferrer">
              {item.url}
            </a>
          ) : null}
        </div>
      ))}
    </div>
  )
}

function SearchTrail({ traces }: { traces: SearchTrace[] }) {
  if (traces.length === 0) {
    return <p className="muted-copy">暂无搜索记录。</p>
  }

  return (
    <div className="trace-list">
      {traces.map((trace) => (
        <div key={trace.id} className={`trace-row ${trace.status}`}>
          <div>
            <strong>{trace.query}</strong>
            <span>{trace.status}</span>
          </div>
          {trace.error ? <p>{trace.error}</p> : null}
          {trace.sources.map((source) => (
            <a key={source.url} href={source.url} target="_blank" rel="noreferrer">
              {source.title}
            </a>
          ))}
        </div>
      ))}
    </div>
  )
}

function makeMeetingTitle(digest: MeetingDigest, segments: MeetingSegment[], createdAt: string): string {
  const seed = digest.text || segments[0]?.text || `Meeting ${formatClock(createdAt)}`
  const normalized = seed.replace(/\s+/g, ' ').trim()
  return normalized.length > 28 ? `${normalized.slice(0, 28)}...` : normalized
}

function wakePhraseList(raw: string): string[] {
  return raw
    .split(',')
    .map((phrase) => phrase.trim())
    .filter(Boolean)
}

function extractVoiceTrigger(text: string, phrases: string[]): VoiceTrigger | null {
  const normalizedText = text.toLowerCase()

  for (const phrase of phrases) {
    const normalizedPhrase = phrase.toLowerCase()
    const index = normalizedText.indexOf(normalizedPhrase)

    if (index < 0) {
      continue
    }

    const beforeWake = text
      .slice(0, index)
      .replace(/[\s,，。.!！?？:：、]+$/, '')
      .trim()
    const afterWake = text
      .slice(index + phrase.length)
      .replace(/^[\s,，。.!！?？:：、]+/, '')
      .trim()

    return {
      phrase,
      transcript: text,
      question: afterWake,
      beforeText: beforeWake,
    }
  }

  return null
}

function buildWakeCommand(capture: WakeCapture): string {
  return capture.parts.join(' ').replace(/\s+/g, ' ').trim()
}

function digestStatusLabel(
  status: DigestStatus,
  digest: MeetingDigest,
  pendingSegments: number,
  pendingAsrChunks: number,
): string {
  if (status === 'updating') {
    return pendingSegments > 0 ? `Updating digest (${pendingSegments} new segments)` : 'Updating digest'
  }
  if (status === 'pending') {
    return pendingSegments > 0 ? `Digest queued (${pendingSegments} new segments)` : 'Digest queued'
  }
  if (status === 'error') {
    return digest.error ? `Digest error: ${digest.error}` : 'Digest error'
  }
  if (pendingAsrChunks > 0) {
    return `Waiting for ASR (${pendingAsrChunks} streaming items)`
  }
  if (digest.updatedAt) {
    return `Updated ${new Date(digest.updatedAt).toLocaleTimeString()}`
  }
  return 'Waiting for transcript'
}

function speechStatusLabel(status: TranscriptionStatus): string {
  switch (status) {
    case 'connecting':
      return 'Connecting'
    case 'listening':
      return 'Listening'
    case 'finalizing':
      return 'Finalizing'
    case 'error':
      return 'Error'
    case 'idle':
    default:
      return 'Idle'
  }
}

function resampleLinear(samples: Float32Array, inputSampleRate: number, outputSampleRate: number): Float32Array {
  if (inputSampleRate === outputSampleRate) {
    return samples
  }

  const outputLength = Math.max(1, Math.round((samples.length * outputSampleRate) / inputSampleRate))
  const output = new Float32Array(outputLength)
  const ratio = (samples.length - 1) / Math.max(1, outputLength - 1)

  for (let index = 0; index < outputLength; index += 1) {
    const position = index * ratio
    const lower = Math.floor(position)
    const upper = Math.min(samples.length - 1, lower + 1)
    const weight = position - lower
    output[index] = samples[lower] * (1 - weight) + samples[upper] * weight
  }

  return output
}

function pcm16ToBase64(samples: Float32Array): string {
  const bytes = new Uint8Array(samples.length * 2)
  const view = new DataView(bytes.buffer)

  for (let index = 0; index < samples.length; index += 1) {
    const clamped = Math.max(-1, Math.min(1, samples[index]))
    view.setInt16(index * 2, clamped < 0 ? clamped * 0x8000 : clamped * 0x7fff, true)
  }

  return bytesToBase64(bytes)
}

function bytesToBase64(bytes: Uint8Array): string {
  let binary = ''
  for (let index = 0; index < bytes.length; index += 1) {
    binary += String.fromCharCode(bytes[index])
  }
  return window.btoa(binary)
}

export default App
