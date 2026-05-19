import * as OpenCC from 'opencc-js/t2cn'

const taiwanToSimplified = OpenCC.Converter({ from: 'tw', to: 'cn' })
const hongKongToSimplified = OpenCC.Converter({ from: 'hk', to: 'cn' })

export function toSimplifiedChinese(text: string): string {
  return hongKongToSimplified(taiwanToSimplified(text))
}

export function normalizeTranscriptText(text: string): string {
  return toSimplifiedChinese(text).replace(/\s+/g, ' ').trim()
}
