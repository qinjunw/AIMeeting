import type { SpeechRecognitionSupport } from '../types'

type SpeechRecognitionAlternative = {
  transcript: string
  confidence: number
}

type SpeechRecognitionResultLike = {
  isFinal: boolean
  0: SpeechRecognitionAlternative
}

type SpeechRecognitionResultListLike = {
  length: number
  item(index: number): SpeechRecognitionResultLike
  [index: number]: SpeechRecognitionResultLike
}

type SpeechRecognitionEventLike = Event & {
  resultIndex: number
  results: SpeechRecognitionResultListLike
}

type SpeechRecognitionErrorEventLike = Event & {
  error?: string
  message?: string
}

type SpeechRecognitionLike = EventTarget & {
  continuous: boolean
  interimResults: boolean
  lang: string
  maxAlternatives: number
  onstart: (() => void) | null
  onend: (() => void) | null
  onerror: ((event: SpeechRecognitionErrorEventLike) => void) | null
  onresult: ((event: SpeechRecognitionEventLike) => void) | null
  start(): void
  stop(): void
  abort(): void
}

type SpeechRecognitionConstructor = new () => SpeechRecognitionLike

type WindowWithSpeechRecognition = Window & {
  SpeechRecognition?: SpeechRecognitionConstructor
  webkitSpeechRecognition?: SpeechRecognitionConstructor
}

export type SpeechSessionHandlers = {
  lang: string
  onStart(): void
  onEnd(): void
  onInterim(text: string): void
  onFinal(text: string, confidence: number): void
  onError(message: string): void
}

export type SpeechSession = {
  start(): void
  stop(): void
  abort(): void
}

export function getSpeechRecognitionSupport(): SpeechRecognitionSupport {
  const recognizer = getSpeechRecognitionConstructor()

  if (!recognizer) {
    return {
      supported: false,
      label: 'SpeechRecognition unavailable',
      detail: '当前 WebView/浏览器没有暴露 SpeechRecognition。可以先用 Chrome/Edge 运行 Web 原型，后续接云端 ASR。',
    }
  }

  return {
    supported: true,
    label: 'Browser speech recognition ready',
    detail: '可以使用麦克风做实时听写。识别能力由当前浏览器/WebView 提供。',
  }
}

export function createSpeechSession(handlers: SpeechSessionHandlers): SpeechSession | null {
  const Recognition = getSpeechRecognitionConstructor()

  if (!Recognition) {
    return null
  }

  const recognition = new Recognition()
  recognition.continuous = true
  recognition.interimResults = true
  recognition.lang = handlers.lang
  recognition.maxAlternatives = 1

  recognition.onstart = handlers.onStart
  recognition.onend = handlers.onEnd
  recognition.onerror = (event) => {
    const code = event.error ? ` (${event.error})` : ''
    handlers.onError(`${event.message || 'Speech recognition failed'}${code}`)
  }
  recognition.onresult = (event) => {
    let interim = ''
    const finals: Array<{ text: string; confidence: number }> = []

    for (let index = event.resultIndex; index < event.results.length; index += 1) {
      const result = event.results[index] ?? event.results.item(index)
      const alternative = result[0]

      if (!alternative?.transcript) {
        continue
      }

      if (result.isFinal) {
        finals.push({
          text: alternative.transcript.trim(),
          confidence: alternative.confidence || 0.85,
        })
      } else {
        interim += alternative.transcript
      }
    }

    handlers.onInterim(interim.trim())
    for (const finalResult of finals) {
      handlers.onFinal(finalResult.text, finalResult.confidence)
    }
  }

  return {
    start: () => recognition.start(),
    stop: () => recognition.stop(),
    abort: () => recognition.abort(),
  }
}

function getSpeechRecognitionConstructor(): SpeechRecognitionConstructor | undefined {
  const target = window as WindowWithSpeechRecognition
  return target.SpeechRecognition ?? target.webkitSpeechRecognition
}
