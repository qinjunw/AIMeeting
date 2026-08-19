import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

export type Unsubscribe = () => void

export type DesktopTransport = {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>
  listen<T>(eventName: string, handler: (payload: T) => void): Promise<Unsubscribe>
}

export const tauriTransport: DesktopTransport = {
  invoke<T>(command: string, args?: Record<string, unknown>) {
    if (!isTauriRuntime()) {
      if (['list_meetings', 'list_trash', 'list_provider_profiles'].includes(command)) {
        return Promise.resolve([] as T)
      }
      if (command === 'get_active_meeting') return Promise.resolve(null as T)
      return Promise.reject(new Error('此操作需要在 AIMeeting 桌面应用中运行。'))
    }
    return invoke<T>(command, args)
  },
  listen<T>(eventName: string, handler: (payload: T) => void) {
    if (!isTauriRuntime()) return Promise.resolve(() => undefined)
    return listen<T>(eventName, (event) => handler(event.payload))
  },
}

function isTauriRuntime() {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}
