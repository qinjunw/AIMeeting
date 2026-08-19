// @vitest-environment jsdom

import { act, renderHook, waitFor } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import type {
  ProviderClient,
  ProviderProfile,
  SaveProviderProfileRequest,
} from '../../bridge/providerClient'
import { useProviderSettings } from './useProviderSettings'

const profile: ProviderProfile = {
  id: 'provider-1',
  capability: 'live_transcription',
  name: 'DashScope 实时转写',
  baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode/v1',
  model: 'paraformer-realtime-v2',
  endpointFlavor: 'realtime-websocket',
  hasSecret: true,
  isDefault: true,
}

describe('useProviderSettings', () => {
  it('loads and saves non-secret profiles while passing the secret only to the bridge', async () => {
    const client: ProviderClient = {
      listProfiles: vi.fn().mockResolvedValue([]),
      saveProfile: vi.fn().mockResolvedValue(profile),
      deleteProfile: vi.fn(),
      testProfile: vi.fn().mockResolvedValue({
        providerId: 'provider-1',
        detail: '连接成功',
      }),
    }
    const { result } = renderHook(() => useProviderSettings(client))
    await waitFor(() => expect(result.current.status).toBe('ready'))

    const request: SaveProviderProfileRequest = {
      id: 'provider-1',
      capability: 'live_transcription',
      name: 'DashScope 实时转写',
      baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode/v1',
      model: 'paraformer-realtime-v2',
      endpointFlavor: 'realtime-websocket',
      apiKey: 'secret-value',
      isDefault: true,
    }
    await act(async () => result.current.saveProfile(request))

    expect(client.saveProfile).toHaveBeenCalledWith(request)
    expect(result.current.profiles).toEqual([profile])
    expect('apiKey' in result.current.profiles[0]).toBe(false)
  })
})
