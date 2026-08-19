import { useCallback, useEffect, useRef, useState } from 'react'

import {
  providerClient as defaultProviderClient,
  type ProviderClient,
  type ProviderProfile,
  type ProviderTestResult,
  type SaveProviderProfileRequest,
} from '../../bridge/providerClient'

export type ProviderSettingsStatus = 'idle' | 'loading' | 'ready' | 'error'

export function useProviderSettings(
  client: ProviderClient = defaultProviderClient,
) {
  const [profiles, setProfiles] = useState<ProviderProfile[]>([])
  const [status, setStatus] = useState<ProviderSettingsStatus>('idle')
  const [error, setError] = useState<string | null>(null)
  const [testingProfileId, setTestingProfileId] = useState<string | null>(null)
  const [testResult, setTestResult] = useState<ProviderTestResult | null>(null)
  const initialized = useRef(false)
  const loadSequence = useRef(0)

  const refresh = useCallback(async () => {
    const sequence = ++loadSequence.current
    setStatus('loading')
    setError(null)
    try {
      const nextProfiles = await client.listProfiles()
      if (sequence !== loadSequence.current) return
      setProfiles(nextProfiles)
      setStatus('ready')
    } catch (loadError) {
      if (sequence !== loadSequence.current) return
      setStatus('error')
      setError(errorMessage(loadError))
    }
  }, [client])

  useEffect(() => {
    if (initialized.current) return
    initialized.current = true
    void refresh()
  }, [refresh])

  const saveProfile = useCallback(
    async (request: SaveProviderProfileRequest) => {
      setError(null)
      const saved = await client.saveProfile(request)
      setProfiles((current) => {
        const exists = current.some((profile) => profile.id === saved.id)
        return exists
          ? current.map((profile) => (profile.id === saved.id ? saved : profile))
          : [...current, saved]
      })
      return saved
    },
    [client],
  )

  const deleteProfile = useCallback(
    async (profileId: string) => {
      setError(null)
      await client.deleteProfile(profileId)
      setProfiles((current) => current.filter((profile) => profile.id !== profileId))
      if (testingProfileId === profileId) {
        setTestingProfileId(null)
        setTestResult(null)
      }
    },
    [client, testingProfileId],
  )

  const testProfile = useCallback(
    async (profileId: string) => {
      setTestingProfileId(profileId)
      setTestResult(null)
      setError(null)
      try {
        const result = await client.testProfile(profileId)
        setTestResult(result)
        return result
      } catch (testError) {
        setError(errorMessage(testError))
        throw testError
      } finally {
        setTestingProfileId(null)
      }
    },
    [client],
  )

  return {
    profiles,
    status,
    error,
    testingProfileId,
    testResult,
    refresh,
    saveProfile,
    deleteProfile,
    testProfile,
  }
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}
