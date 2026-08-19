import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

export type Unsubscribe = () => void

export type DesktopTransport = {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>
  listen<T>(eventName: string, handler: (payload: T) => void): Promise<Unsubscribe>
}

export const tauriTransport: DesktopTransport = {
  invoke<T>(command: string, args?: Record<string, unknown>) {
    return invoke<T>(command, args)
  },
  listen<T>(eventName: string, handler: (payload: T) => void) {
    return listen<T>(eventName, (event) => handler(event.payload))
  },
}
