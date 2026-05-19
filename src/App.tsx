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
import { getAsrRuntimeStatus, transcribeAudioChunk } from './services/asrTranscription'
import { normalizeTranscriptText } from './services/chineseText'
import { generateAgentDraft, generateMeetingDigest } from './services/modelProvider'
import { runAutoSearch } from './services/searchTool'
import { createSpeechSession, getSpeechRecognitionSupport } from './services/speechRecognition'
import type { SpeechSession } from './services/speechRecognition'
import type {
  AgentResponse,
  AsrProviderConfig,
  AsrRuntimeStatus,
  AsrTranscriptionResponse,
  AudioSource,
  CaptureProbe,
  MeetingDigest,
  MeetingMode,
  MeetingRecord,
  MeetingSegment,
  ProviderConfig,
  SearchConfig,
  SearchTrace,
  SpeechRecognitionStatus,
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
  baseUrl: '',
  apiKey: '',
  model: '',
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
const asrChunkSeconds = 2
const wakeSilenceMs = 4000
const digestUpdateMs = 4000
const speechRmsThreshold = 0.006
const speechPeakThreshold = 0.02

const emptyDigest: MeetingDigest = {
  text: '',
  updatedAt: '',
  providerLabel: '',
  segmentCount: 0,
}

type DigestStatus = 'idle' | 'pending' | 'updating' | 'error'

type WakeCapture = {
  phrase: string
  transcript: string
  parts: string[]
  startedAt: string
}

type WindowWithAudioContext = Window & {
  webkitAudioContext?: typeof AudioContext
}

function App() {
  const initialSpeechSupport = getSpeechRecognitionSupport()
  const [mode, setMode] = useState<MeetingMode>('paused')
  const [activeMeetingId, setActiveMeetingId] = useState(() => loadJson(activeMeetingIdStorageKey, makeId('meeting')))
  const [activeMeetingCreatedAt, setActiveMeetingCreatedAt] = useState(() =>
    loadJson(activeMeetingCreatedAtStorageKey, new Date().toISOString()),
  )
  const [segments, setSegments] = useState<MeetingSegment[]>(() => loadJson(segmentsStorageKey, []))
  const [responses, setResponses] = useState<AgentResponse[]>(() => loadJson(responsesStorageKey, []))
  const [meetings, setMeetings] = useState<MeetingRecord[]>(() => loadJson(meetingsStorageKey, []))
  const [selectedHistoryId, setSelectedHistoryId] = useState<string | null>(null)
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
  const [isThinking, setIsThinking] = useState(false)
  const [speechSupport, setSpeechSupport] = useState(initialSpeechSupport)
  const [speechStatus, setSpeechStatus] = useState<SpeechRecognitionStatus>(
    initialSpeechSupport.supported ? 'idle' : 'unsupported',
  )
  const [speechLang, setSpeechLang] = useState(() => loadJson(speechLangStorageKey, 'zh-CN'))
  const [wakePhrases, setWakePhrases] = useState(() => loadJson(wakePhrasesStorageKey, '嗨助手,嘿助手,助手,hey assistant'))
  const [interimTranscript, setInterimTranscript] = useState('')
  const [lastVoiceTrigger, setLastVoiceTrigger] = useState<VoiceTrigger | null>(null)
  const [autoAskOnWake, setAutoAskOnWake] = useState(false)
  const [asrRuntime, setAsrRuntime] = useState<AsrRuntimeStatus | null>(null)
  const [meetingDigest, setMeetingDigest] = useState<MeetingDigest>(() => loadJson(digestStorageKey, emptyDigest))
  const [digestStatus, setDigestStatus] = useState<DigestStatus>('idle')

  const speechSessionRef = useRef<SpeechSession | null>(null)
  const keepListeningRef = useRef(false)
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
  const asrQueuesRef = useRef<Map<string, Promise<void>>>(new Map())
  const asrPendingCountRef = useRef<Map<string, number>>(new Map())
  const wakeCaptureRef = useRef<WakeCapture | null>(null)
  const wakeSilenceTimerRef = useRef<number | null>(null)
  const digestTimerRef = useRef<number | null>(null)
  const digestRunningRef = useRef(false)

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
    if (!showAdvancedSettings) {
      return
    }

    function closeOnEscape(event: KeyboardEvent) {
      if (event.key === 'Escape') {
        setShowAdvancedSettings(false)
      }
    }

    window.addEventListener('keydown', closeOnEscape)
    return () => window.removeEventListener('keydown', closeOnEscape)
  }, [showAdvancedSettings])

  useEffect(() => {
    void refreshAsrRuntimeStatus()
  }, [])

  useEffect(
    () => () => {
      keepListeningRef.current = false
      speechSessionRef.current?.abort()
      stopChunkedAsr()
      clearWakeSilenceTimer()
      clearDigestTimer()
    },
    [],
  )

  function setMeetingsState(updater: (current: MeetingRecord[]) => MeetingRecord[]) {
    setMeetings((current) => {
      const nextMeetings = updater(current)
      meetingsRef.current = nextMeetings
      return nextMeetings
    })
  }

  function updateHistoricalMeeting(meetingId: string, updater: (meeting: MeetingRecord) => MeetingRecord) {
    setMeetingsState((current) => current.map((meeting) => (meeting.id === meetingId ? updater(meeting) : meeting)))
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
    stopChunkedAsr(meetingId)
    cancelWakeCapture()
    setQuestion('')
    setMode('paused')
  }

  function clearMeeting() {
    keepListeningRef.current = false
    speechSessionRef.current?.abort()
    speechSessionRef.current = null
    stopChunkedAsr(activeMeetingIdRef.current, { flushFinal: false })
    clearWakeSilenceTimer()
    wakeCaptureRef.current = null
    clearDigestTimer()
    asrPendingCountRef.current.delete(activeMeetingIdRef.current)
    asrQueuesRef.current.delete(activeMeetingIdRef.current)
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
    setSpeechStatus(speechSupport.supported ? 'idle' : 'unsupported')
    setMode('paused')
  }

  async function refreshAsrRuntimeStatus() {
    try {
      const status = await getAsrRuntimeStatus()
      setAsrRuntime(status)
    } catch (error) {
      setCaptureLog((current) => [
        {
          ok: false,
          label: 'ASR runtime check failed',
          detail: error instanceof Error ? error.message : '无法读取本地 ASR 运行状态。',
        },
        ...current,
      ].slice(0, 4))
    }
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

    isThinkingRef.current = true
    setIsThinking(true)
    setMode('searching')

    const started = performance.now()
    const searches = await runAutoSearch(trimmed, contextSegments, searchConfig)
    const draft = await generateAgentDraft({ question: trimmed, segments: contextSegments, searches, provider })
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

  function clearDigestTimer() {
    if (digestTimerRef.current) {
      window.clearTimeout(digestTimerRef.current)
      digestTimerRef.current = null
    }
  }

  function scheduleDigestUpdate() {
    if (digestRunningRef.current || digestTimerRef.current) {
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
    }, digestUpdateMs)
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

    if (meetingId === activeMeetingIdRef.current && !result.error && segmentsRef.current.length > nextDigest.segmentCount) {
      scheduleDigestUpdate()
    }
  }

  async function finalizeArchivedMeeting(meetingId: string) {
    await (asrQueuesRef.current.get(meetingId) ?? Promise.resolve())

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

  async function startChunkedAsr() {
    if (asrRecordingRef.current) {
      return
    }

    const meetingId = activeMeetingIdRef.current
    keepListeningRef.current = false
    speechSessionRef.current?.abort()
    speechSessionRef.current = null
    pcmBufferRef.current = []
    pcmSampleCountRef.current = 0
    asrRecordingRef.current = true
    setSpeechStatus('listening')
    setMode('recording')
    setInterimTranscript(`正在采集音频，约每 ${asrChunkSeconds} 秒转写一次。`)

    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true })
      const AudioContextCtor = window.AudioContext || (window as WindowWithAudioContext).webkitAudioContext
      if (!AudioContextCtor) {
        throw new Error('当前 WebView/浏览器不支持 AudioContext。')
      }
      const audioContext = new AudioContextCtor()
      const source = audioContext.createMediaStreamSource(stream)
      const processor = audioContext.createScriptProcessor(4096, 1, 1)
      const chunkSamples = audioContext.sampleRate * asrChunkSeconds

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

        if (pcmSampleCountRef.current >= chunkSamples) {
          const audio = flushWavChunk(audioContext.sampleRate)
          if (audio) {
            enqueueAsrChunk(audio, meetingId)
          }
        }
      }

      source.connect(processor)
      processor.connect(audioContext.destination)
      mediaStreamRef.current = stream
      audioContextRef.current = audioContext
      audioSourceRef.current = source
      audioProcessorRef.current = processor

      setCaptureLog((current) => [
        {
          ok: true,
          label: 'Chunked ASR started',
          detail: asrProvider.apiKey.trim()
            ? `云端 ASR 优先，失败后自动切换本地 Whisper。语言：${speechLang}。`
            : `使用本地 Whisper + Silero VAD 分段转写。语言：${speechLang}。`,
        },
        ...current,
      ].slice(0, 4))
      await refreshAsrRuntimeStatus()
    } catch (error) {
      asrRecordingRef.current = false
      stopAudioGraph()
      setSpeechStatus('error')
      setInterimTranscript('')
      setCaptureLog((current) => [
        {
          ok: false,
          label: 'Chunked ASR start failed',
          detail: error instanceof Error ? error.message : '无法启动分段 ASR。',
        },
        ...current,
      ].slice(0, 4))
    }
  }

  function stopChunkedAsr(
    meetingId = activeMeetingIdRef.current,
    options: { flushFinal?: boolean } = { flushFinal: true },
  ) {
    const wasRecording = asrRecordingRef.current
    const sampleRate = audioContextRef.current?.sampleRate ?? 16000
    asrRecordingRef.current = false

    if (wasRecording && options.flushFinal !== false) {
      const audio = flushWavChunk(sampleRate)
      if (audio) {
        enqueueAsrChunk(audio, meetingId)
      }
    }

    stopAudioGraph()
    setInterimTranscript('')
    setSpeechStatus('idle')
  }

  function enqueueAsrChunk(audio: Blob, meetingId = activeMeetingIdRef.current) {
    const providerSnapshot = asrProvider
    const languageSnapshot = speechLang
    const previousQueue = asrQueuesRef.current.get(meetingId) ?? Promise.resolve()

    if (meetingId === activeMeetingIdRef.current) {
      setInterimTranscript('正在转写刚才的音频片段...')
    }
    asrPendingCountRef.current.set(meetingId, (asrPendingCountRef.current.get(meetingId) ?? 0) + 1)

    const nextQueue = previousQueue
      .then(() => transcribeAudioChunk({ audio, provider: providerSnapshot, language: languageSnapshot }))
      .then((result) => handleAsrResult(result, meetingId))
      .catch((error) => {
        if (meetingId === activeMeetingIdRef.current) {
          setSpeechStatus('error')
        } else {
          updateHistoricalMeeting(meetingId, (meeting) => ({
            ...meeting,
            status: 'error',
            error: error instanceof Error ? error.message : '音频转写失败。',
            updatedAt: new Date().toISOString(),
          }))
        }
        setCaptureLog((current) => [
          {
            ok: false,
            label: 'ASR transcription failed',
            detail: error instanceof Error ? error.message : '音频转写失败。',
          },
          ...current,
        ].slice(0, 4))
      })
      .finally(() => {
        const nextPending = Math.max(0, (asrPendingCountRef.current.get(meetingId) ?? 1) - 1)
        asrPendingCountRef.current.set(meetingId, nextPending)
        if (meetingId === activeMeetingIdRef.current) {
          setInterimTranscript((current) => (current === '正在转写刚才的音频片段...' ? '' : current))
        }
      })
    asrQueuesRef.current.set(meetingId, nextQueue)
  }

  function handleAsrResult(result: AsrTranscriptionResponse, meetingId = activeMeetingIdRef.current) {
    setAsrRuntime((current) => ({
      ...(current ?? { localReady: false, runtimeLabel: 'whisper.cpp + Silero VAD' }),
      localReady: Boolean(result.localServerUrl) || current?.localReady || false,
      localServerUrl: result.localServerUrl ?? current?.localServerUrl,
    }))
    const text = result.text.trim()
    if (!text) {
      return
    }

    if (meetingId === activeMeetingIdRef.current) {
      handleRecognizedTranscript(text, 0.88, 'Me', 'microphone')
    } else {
      appendHistoricalTranscriptSegment(meetingId, text, 0.88, 'Me', 'microphone')
    }
    setCaptureLog((current) => [
      {
        ok: !result.warning,
        label: result.usedFallback ? 'ASR fallback used' : 'ASR segment transcribed',
        detail: result.warning ?? `${result.providerLabel} 已处理 1 段音频文本。`,
      },
      ...current,
    ].slice(0, 4))
  }

  function flushWavChunk(sampleRate: number): Blob | null {
    const sampleCount = pcmSampleCountRef.current
    if (sampleCount < sampleRate * 0.6) {
      return null
    }

    const samples = mergeSamples(pcmBufferRef.current, sampleCount)
    pcmBufferRef.current = []
    pcmSampleCountRef.current = 0
    if (!hasSpeechEnergy(samples)) {
      return null
    }
    return encodeWav(samples, sampleRate)
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

  function startLegacySpeechRecognition() {
    const support = getSpeechRecognitionSupport()
    const meetingId = activeMeetingIdRef.current
    setSpeechSupport(support)

    if (!support.supported) {
      setSpeechStatus('unsupported')
      setCaptureLog((current) => [
        {
          ok: false,
          label: support.label,
          detail: support.detail,
        },
        ...current,
      ].slice(0, 4))
      return
    }

    speechSessionRef.current?.abort()
    keepListeningRef.current = true
    setSpeechStatus('listening')
    setMode('recording')

    const session = createSpeechSession({
      lang: speechLang,
      onStart: () => {
        setSpeechStatus('listening')
        setCaptureLog((current) => [
          {
            ok: true,
            label: 'Mic transcription started',
            detail: `正在使用 ${speechLang} 实时听写。`,
          },
          ...current,
        ].slice(0, 4))
      },
      onEnd: () => {
        setInterimTranscript('')
        if (keepListeningRef.current) {
          window.setTimeout(() => {
            try {
              speechSessionRef.current?.start()
            } catch (error) {
              keepListeningRef.current = false
              setSpeechStatus('error')
              setCaptureLog((current) => [
                {
                  ok: false,
                  label: 'Mic transcription restart failed',
                  detail: error instanceof Error ? error.message : '无法重新启动实时听写。',
                },
                ...current,
              ].slice(0, 4))
            }
          }, 350)
          return
        }

        setSpeechStatus(speechSupport.supported ? 'idle' : 'unsupported')
      },
      onInterim: (text) => {
        if (meetingId === activeMeetingIdRef.current) {
          setInterimTranscript(text)
        }
      },
      onFinal: (text, confidence) => handleFinalTranscript(meetingId, text, confidence),
      onError: (message) => {
        setSpeechStatus('error')
        setCaptureLog((current) => [
          {
            ok: false,
            label: 'Mic transcription error',
            detail: message,
          },
          ...current,
        ].slice(0, 4))

        if (/not-allowed|permission|service-not-allowed/i.test(message)) {
          keepListeningRef.current = false
        }
      },
    })

    if (!session) {
      setSpeechStatus('unsupported')
      return
    }

    speechSessionRef.current = session

    try {
      session.start()
    } catch (error) {
      keepListeningRef.current = false
      setSpeechStatus('error')
      setCaptureLog((current) => [
        {
          ok: false,
          label: 'Mic transcription start failed',
          detail: error instanceof Error ? error.message : '无法启动实时听写。',
        },
        ...current,
      ].slice(0, 4))
    }
  }

  function stopLegacySpeechRecognition() {
    keepListeningRef.current = false
    setSpeechStatus('stopping')
    setInterimTranscript('')
    speechSessionRef.current?.stop()
  }

  function stopActiveTranscription() {
    const meetingId = activeMeetingIdRef.current
    if (asrRecordingRef.current) {
      stopChunkedAsr(meetingId)
    } else if (speechStatus === 'listening') {
      stopLegacySpeechRecognition()
    } else {
      setSpeechStatus('idle')
    }

    cancelWakeCapture()
    if (segmentsRef.current.length > 0 || responsesRef.current.length > 0 || meetingDigestRef.current.text) {
      archiveActiveMeeting(meetingId)
    }
    resetActiveMeeting('paused')
  }

  function handleFinalTranscript(meetingId: string, text: string, confidence: number) {
    if (!text.trim()) {
      return
    }

    if (meetingId === activeMeetingIdRef.current) {
      handleRecognizedTranscript(text, confidence, 'Me', 'microphone')
    } else {
      appendHistoricalTranscriptSegment(meetingId, text, confidence, 'Me', 'microphone')
    }
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
  const digestMeta = digestStatusLabel(digestStatus, meetingDigest)
  const activeMeetingTitle = makeMeetingTitle(meetingDigest, segments, activeMeetingCreatedAt)
  const providerSummary = hasTextProvider(provider) ? `Agent ${provider.model}` : 'Agent not configured'
  const asrSummary = asrProvider.apiKey.trim() ? `Cloud ASR ${asrProvider.model || 'configured'}` : 'Local Whisper'
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
          {meetings.length === 0 ? (
            <p className="muted-copy">Stop 后会在这里保存上一段会议。</p>
          ) : (
            <div className="history-list">
              {meetings.slice(0, 8).map((meeting) => (
                <button
                  type="button"
                  key={meeting.id}
                  className={`history-item ${selectedHistoryId === meeting.id ? 'selected' : ''}`}
                  onClick={() => setSelectedHistoryId((current) => (current === meeting.id ? null : meeting.id))}
                >
                  <span>{meeting.title}</span>
                  <small>
                    {meeting.status} · {formatClock(meeting.stoppedAt)}
                  </small>
                </button>
              ))}
            </div>
          )}
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
              {asrProvider.apiKey.trim()
                ? 'Cloud ASR first; local Whisper fallback.'
                : `${asrRuntime?.runtimeLabel ?? 'Local Whisper'}${
                    asrRuntime?.localServerUrl ? ` on ${asrRuntime.localServerUrl}` : ' ready on demand.'
                  }`}
            </small>
          </div>
          <div className="voice-actions">
            <button
              type="button"
              className="icon-command primary"
              onClick={startChunkedAsr}
              disabled={speechStatus === 'listening'}
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
              disabled={speechStatus !== 'listening' && segments.length === 0 && !meetingDigest.text}
            >
              <Square size={17} />
              <span>Stop</span>
            </button>
          </div>
          <button
            type="button"
            className="icon-command"
            onClick={startLegacySpeechRecognition}
            disabled={speechStatus === 'listening' || !speechSupport.supported}
            title={speechSupport.detail}
          >
            <Languages size={17} />
            <span>Legacy Web Speech</span>
          </button>
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
                  <p>点击 Start mic 开始记录，或手动添加一段测试文本。</p>
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
            placeholder="输入问题，或说：嗨助手 总结刚才内容"
          />
          <button type="submit" className="send-button" disabled={isThinking || !question.trim()}>
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
                    placeholder="leave empty for local Whisper"
                  />
                </label>
                <label>
                  <span>Model</span>
                  <input
                    value={asrProvider.model}
                    onChange={(event) => setAsrProvider((current) => ({ ...current, model: event.target.value }))}
                    placeholder="whisper-1"
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
                <button type="button" className="icon-command" onClick={refreshAsrRuntimeStatus}>
                  <Activity size={17} />
                  <span>Check local ASR</span>
                </button>
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

function digestStatusLabel(status: DigestStatus, digest: MeetingDigest): string {
  if (status === 'updating') {
    return 'Updating digest'
  }
  if (status === 'pending') {
    return 'Digest queued'
  }
  if (status === 'error') {
    return digest.error ? `Digest error: ${digest.error}` : 'Digest error'
  }
  if (digest.updatedAt) {
    return `Updated ${new Date(digest.updatedAt).toLocaleTimeString()}`
  }
  return 'Waiting for transcript'
}

function speechStatusLabel(status: SpeechRecognitionStatus): string {
  switch (status) {
    case 'listening':
      return 'Listening'
    case 'stopping':
      return 'Stopping'
    case 'error':
      return 'Error'
    case 'unsupported':
      return 'Unsupported'
    case 'idle':
    default:
      return 'Idle'
  }
}

function mergeSamples(chunks: Float32Array[], sampleCount: number): Float32Array {
  const merged = new Float32Array(sampleCount)
  let offset = 0

  for (const chunk of chunks) {
    merged.set(chunk, offset)
    offset += chunk.length
  }

  return merged
}

function hasSpeechEnergy(samples: Float32Array): boolean {
  let sumSquares = 0
  let peak = 0

  for (const sample of samples) {
    const value = Math.abs(sample)
    sumSquares += value * value
    peak = Math.max(peak, value)
  }

  const rms = Math.sqrt(sumSquares / Math.max(1, samples.length))
  return rms >= speechRmsThreshold || peak >= speechPeakThreshold
}

function encodeWav(samples: Float32Array, sampleRate: number): Blob {
  const bytesPerSample = 2
  const channels = 1
  const dataSize = samples.length * bytesPerSample
  const buffer = new ArrayBuffer(44 + dataSize)
  const view = new DataView(buffer)

  writeAscii(view, 0, 'RIFF')
  view.setUint32(4, 36 + dataSize, true)
  writeAscii(view, 8, 'WAVE')
  writeAscii(view, 12, 'fmt ')
  view.setUint32(16, 16, true)
  view.setUint16(20, 1, true)
  view.setUint16(22, channels, true)
  view.setUint32(24, sampleRate, true)
  view.setUint32(28, sampleRate * channels * bytesPerSample, true)
  view.setUint16(32, channels * bytesPerSample, true)
  view.setUint16(34, 16, true)
  writeAscii(view, 36, 'data')
  view.setUint32(40, dataSize, true)

  let offset = 44
  for (const sample of samples) {
    const clamped = Math.max(-1, Math.min(1, sample))
    view.setInt16(offset, clamped < 0 ? clamped * 0x8000 : clamped * 0x7fff, true)
    offset += 2
  }

  return new Blob([buffer], { type: 'audio/wav' })
}

function writeAscii(view: DataView, offset: number, value: string) {
  for (let index = 0; index < value.length; index += 1) {
    view.setUint8(offset + index, value.charCodeAt(index))
  }
}

export default App
