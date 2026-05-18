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
  Play,
  Plus,
  Radio,
  Search,
  Send,
  Settings,
  Shield,
  Sparkles,
  Square,
  Trash2,
} from 'lucide-react'
import { demoLiveSegments, demoSegments } from './data/demoMeeting'
import { formatClock, formatDuration, makeId } from './lib/time'
import { loadJson, saveJson } from './lib/storage'
import { probeMicrophone, probeSystemAudio } from './services/audioCapture'
import { buildRollingSummary } from './services/meetingMemory'
import { generateAgentDraft } from './services/modelProvider'
import { runAutoSearch } from './services/searchTool'
import { createSpeechSession, getSpeechRecognitionSupport } from './services/speechRecognition'
import type { SpeechSession } from './services/speechRecognition'
import type {
  AgentResponse,
  AudioSource,
  CaptureProbe,
  MeetingMode,
  MeetingSegment,
  ProviderConfig,
  SearchConfig,
  SearchTrace,
  SpeechRecognitionStatus,
  VoiceTrigger,
} from './types'

const providerStorageKey = 'aimeeting.provider'
const searchStorageKey = 'aimeeting.search'
const segmentsStorageKey = 'aimeeting.segments'
const responsesStorageKey = 'aimeeting.responses'
const speechLangStorageKey = 'aimeeting.speechLang'
const wakePhrasesStorageKey = 'aimeeting.wakePhrases'

const defaultProvider: ProviderConfig = {
  baseUrl: 'https://api.openai.com/v1',
  apiKey: '',
  model: 'gpt-4.1-mini',
  endpointFlavor: 'chat-completions',
  temperature: 0.25,
}

const defaultSearch: SearchConfig = {
  mode: 'auto',
  endpointTemplate: '',
  redactBeforeSearch: false,
}

const modeMeta: Record<MeetingMode, { label: string; tone: string }> = {
  recording: { label: 'Recording', tone: 'green' },
  'wake-beta': { label: 'Wake beta', tone: 'amber' },
  dialogue: { label: 'Dialogue', tone: 'blue' },
  searching: { label: 'Searching', tone: 'violet' },
  paused: { label: 'Paused', tone: 'muted' },
}

const speakerOptions = ['Speaker A', 'Speaker B', 'Speaker C', 'Me']
const sourceOptions: AudioSource[] = ['system', 'microphone', 'mixed']

function App() {
  const initialSpeechSupport = getSpeechRecognitionSupport()
  const [mode, setMode] = useState<MeetingMode>('recording')
  const [segments, setSegments] = useState<MeetingSegment[]>(() => loadJson(segmentsStorageKey, demoSegments))
  const [responses, setResponses] = useState<AgentResponse[]>(() => loadJson(responsesStorageKey, []))
  const [provider, setProvider] = useState<ProviderConfig>(() => ({
    ...loadJson(providerStorageKey, defaultProvider),
    apiKey: '',
  }))
  const [searchConfig, setSearchConfig] = useState<SearchConfig>(() => loadJson(searchStorageKey, defaultSearch))
  const [question, setQuestion] = useState('刚才讨论的方案有什么风险？帮我整理一个下一步计划。')
  const [manualText, setManualText] = useState('')
  const [manualSpeaker, setManualSpeaker] = useState('Speaker A')
  const [manualSource, setManualSource] = useState<AudioSource>('mixed')
  const [liveIndex, setLiveIndex] = useState(0)
  const [captureLog, setCaptureLog] = useState<CaptureProbe[]>([])
  const [showEvidence, setShowEvidence] = useState(false)
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

  const speechSessionRef = useRef<SpeechSession | null>(null)
  const keepListeningRef = useRef(false)
  const segmentsRef = useRef(segments)
  const isThinkingRef = useRef(false)

  const summary = useMemo(() => buildRollingSummary(segments), [segments])
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

  useEffect(() => {
    segmentsRef.current = segments
    saveJson(segmentsStorageKey, segments)
  }, [segments])

  useEffect(() => {
    saveJson(responsesStorageKey, responses)
  }, [responses])

  useEffect(() => {
    saveJson(searchStorageKey, searchConfig)
  }, [searchConfig])

  useEffect(() => {
    saveJson(providerStorageKey, { ...provider, apiKey: '' })
  }, [provider])

  useEffect(() => {
    saveJson(speechLangStorageKey, speechLang)
  }, [speechLang])

  useEffect(() => {
    saveJson(wakePhrasesStorageKey, wakePhrases)
  }, [wakePhrases])

  useEffect(
    () => () => {
      keepListeningRef.current = false
      speechSessionRef.current?.abort()
    },
    [],
  )

  function toggleRecording() {
    setMode((current) => (current === 'paused' ? 'recording' : 'paused'))
  }

  function clearMeeting() {
    keepListeningRef.current = false
    speechSessionRef.current?.abort()
    speechSessionRef.current = null
    segmentsRef.current = []
    setSegments([])
    setResponses([])
    setInterimTranscript('')
    setLastVoiceTrigger(null)
    setSpeechStatus(speechSupport.supported ? 'idle' : 'unsupported')
    setMode('paused')
  }

  function injectLiveSegment() {
    const template = demoLiveSegments[liveIndex % demoLiveSegments.length]
    const lastEnd = segmentsRef.current.at(-1)?.endMs ?? 0
    const length = template.endMs - template.startMs
    const segment: MeetingSegment = {
      ...template,
      id: makeId('seg'),
      startMs: lastEnd + 1200,
      endMs: lastEnd + 1200 + length,
      createdAt: new Date().toISOString(),
    }
    const nextSegments = [...segmentsRef.current, segment]

    segmentsRef.current = nextSegments
    setSegments(nextSegments)
    setLiveIndex((current) => current + 1)
    setMode('recording')
  }

  function addManualSegment(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const text = manualText.trim()

    if (!text) {
      return
    }

    const nextSegments = appendTranscriptSegment(text, 0.86, manualSpeaker, manualSource)
    setMode('recording')
    handleTranscriptTrigger(text, nextSegments)
    setManualText('')
  }

  async function runAgentQuestion(trimmed: string, contextSegments = segmentsRef.current) {
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

    setResponses((current) => [response, ...current].slice(0, 8))
    setQuestion('')
    setShowEvidence(false)
    setMode('dialogue')
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

  function startSpeechRecognition() {
    const support = getSpeechRecognitionSupport()
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
            detail: `正在使用 ${speechLang} 实时听写。说出“${wakePhraseList(wakePhrases)[0] ?? '嗨助手'}”可以唤起 Agent。`,
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
      onInterim: setInterimTranscript,
      onFinal: handleFinalTranscript,
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

  function stopSpeechRecognition() {
    keepListeningRef.current = false
    setSpeechStatus('stopping')
    setInterimTranscript('')
    speechSessionRef.current?.stop()
  }

  function handleFinalTranscript(text: string, confidence: number) {
    const cleanText = text.trim()

    if (!cleanText) {
      return
    }

    const nextSegments = appendTranscriptSegment(cleanText, confidence, 'Me', 'microphone')
    handleTranscriptTrigger(cleanText, nextSegments)
  }

  function handleTranscriptTrigger(text: string, nextSegments: MeetingSegment[]) {
    const trigger = extractVoiceTrigger(text, wakePhraseList(wakePhrases))

    if (trigger) {
      setLastVoiceTrigger(trigger)
      setMode('dialogue')
      setQuestion(trigger.question)

      if (autoAskOnWake) {
        void runAgentQuestion(trigger.question, nextSegments)
      }
    }
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
    return nextSegments
  }

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

        <section className="control-panel">
          <div className={`mode-pill ${modeMeta[mode].tone}`}>
            <Activity size={16} />
            <span>{modeMeta[mode].label}</span>
          </div>
          <div className="control-grid">
            <button type="button" className="icon-command primary" onClick={toggleRecording} title="Toggle recording">
              {mode === 'paused' ? <Play size={18} /> : <Pause size={18} />}
              <span>{mode === 'paused' ? 'Resume' : 'Pause'}</span>
            </button>
            <button type="button" className="icon-command" onClick={() => setMode('wake-beta')} title="Arm wake beta">
              <Keyboard size={18} />
              <span>Wake beta</span>
            </button>
            <button type="button" className="icon-command" onClick={injectLiveSegment} title="Inject demo segment">
              <Plus size={18} />
              <span>Segment</span>
            </button>
            <button type="button" className="icon-command" onClick={clearMeeting} title="Clear meeting notes">
              <Trash2 size={18} />
              <span>Clear</span>
            </button>
          </div>
        </section>

        <section className="settings-panel voice-panel">
          <div className="panel-title">
            <Mic size={16} />
            <span>Mic transcription</span>
          </div>
          <div className={`speech-state ${speechStatus}`}>
            <span>{speechStatusLabel(speechStatus)}</span>
            <small>{speechSupport.supported ? speechSupport.label : speechSupport.detail}</small>
          </div>
          <div className="voice-actions">
            <button
              type="button"
              className="icon-command primary"
              onClick={startSpeechRecognition}
              disabled={speechStatus === 'listening'}
            >
              <Mic size={18} />
              <span>Start mic</span>
            </button>
            <button
              type="button"
              className="icon-command"
              onClick={stopSpeechRecognition}
              disabled={speechStatus !== 'listening'}
            >
              <Square size={17} />
              <span>Stop</span>
            </button>
          </div>
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
        </section>

        <section className="settings-panel">
          <div className="panel-title">
            <Settings size={16} />
            <span>Provider</span>
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
        </section>

        <section className="settings-panel">
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
              onChange={(event) => setSearchConfig((current) => ({ ...current, endpointTemplate: event.target.value }))}
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
        </section>
      </aside>

      <main className="workspace">
        <header className="topbar">
          <div>
            <p className="eyebrow">Windows-first prototype</p>
            <h2>Live meeting notes</h2>
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
          <Metric icon={FileText} label="Segments" value={segments.length.toString()} />
          <Metric icon={Database} label="Memory" value={`${Math.max(1, Math.round(segments.length * 1.8))} KB`} />
          <Metric icon={Shield} label="Audio retention" value="Transient" />
        </section>

        <section className="memory-band">
          <div className="panel-title">
            <Sparkles size={17} />
            <span>Rolling summary</span>
          </div>
          <p>{summary}</p>
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
                已捕获触发词“{lastVoiceTrigger.phrase}”，Agent 问题已填入：{lastVoiceTrigger.question}
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

        <section className="timeline-section">
          <div className="section-head">
            <div>
              <p className="eyebrow">Transcript event log</p>
              <h2>Segment stream</h2>
            </div>
            <span className="small-badge">{segments.filter((segment) => segment.status === 'final').length} final</span>
          </div>
          <div className="timeline-list">
            {segments
              .slice()
              .reverse()
              .map((segment) => (
                <SegmentRow key={segment.id} segment={segment} />
              ))}
          </div>
        </section>

        <section className="manual-entry">
          <div className="panel-title">
            <Plus size={16} />
            <span>Add transcript segment</span>
          </div>
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
                placeholder="输入一段新的会议 transcript"
              />
              <button type="submit" className="icon-command primary">
                <Plus size={17} />
                <span>Add</span>
              </button>
            </div>
          </form>
        </section>
      </main>

      <aside className="copilot-panel">
        <div className="panel-title large">
          <Bot size={19} />
          <span>Copilot</span>
        </div>
        <form className="ask-box" onSubmit={askAgent}>
          <textarea value={question} onChange={(event) => setQuestion(event.target.value)} rows={5} />
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
            <p>Ask from the current meeting notes.</p>
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
            <p className="muted-copy">No capture events yet.</p>
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
    return <p className="muted-copy">No search trail for the latest answer.</p>
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

    const afterWake = text
      .slice(index + phrase.length)
      .replace(/^[\s,，。.!！?？:：、]+/, '')
      .trim()

    return {
      phrase,
      transcript: text,
      question: afterWake || '请基于当前会议纪要总结重点、待办事项和风险。',
    }
  }

  return null
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

export default App
