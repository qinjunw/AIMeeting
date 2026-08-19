import { tauriTransport, type DesktopTransport, type Unsubscribe } from './transport'

export type ProviderCapability =
  | 'live_transcription'
  | 'file_transcription'
  | 'minutes'

export type ProviderProfile = {
  id: string
  capability: ProviderCapability
  name: string
  baseUrl: string
  model: string
  endpointFlavor: ProviderEndpointFlavor
  hasSecret: boolean
  isDefault: boolean
}

export type SaveProviderProfileRequest = Omit<
  ProviderProfile,
  'id' | 'hasSecret'
> & {
  id: string
  apiKey: string | null
}

export type ProviderEndpointFlavor =
  | 'realtime-websocket'
  | 'audio-transcriptions'
  | 'chat-completions'
  | 'responses'

export type ProviderTestResult = {
  providerId: string
  detail: string
}

export type ProviderClient = {
  listProfiles(): Promise<ProviderProfile[]>
  saveProfile(request: SaveProviderProfileRequest): Promise<ProviderProfile>
  deleteProfile(profileId: string): Promise<void>
  testProfile(profileId: string): Promise<ProviderTestResult>
}

export type ProcessingJobStatus =
  | 'queued'
  | 'running'
  | 'succeeded'
  | 'failed'
  | 'superseded'
export type ProcessingJobType = 'file_transcription' | 'minutes'

export type ProcessingJob = {
  id: string
  meetingId: string
  kind: ProcessingJobType
  status: ProcessingJobStatus
  attempts: number
  inputRevision: number | null
  errorSummary: string | null
}

export type ProcessingJobEvent = ProcessingJob & {
  runGeneration: number
  revision: number
}

export type ProcessingJobClient = {
  list(meetingId?: string): Promise<ProcessingJob[]>
  retryTranscription(meetingId: string): Promise<ProcessingJob>
  retryMinutes(meetingId: string, transcriptRevision: number): Promise<ProcessingJob>
  subscribe(listener: (event: ProcessingJobEvent) => void): Promise<Unsubscribe>
}

export function createProviderClient(
  transport: DesktopTransport = tauriTransport,
): ProviderClient {
  return {
    listProfiles: () => transport.invoke('list_provider_profiles'),
    saveProfile: (request) =>
      transport.invoke('save_provider_profile', { request }),
    deleteProfile: (profileId) =>
      transport.invoke('delete_provider_profile', { request: { profileId } }),
    testProfile: (profileId) =>
      transport.invoke('test_provider_profile', { request: { profileId } }),
  }
}

export function createProcessingJobClient(
  transport: DesktopTransport = tauriTransport,
): ProcessingJobClient {
  return {
    list: (meetingId) =>
      meetingId
        ? transport.invoke('list_processing_jobs', { request: { meetingId } })
        : Promise.resolve([]),
    retryTranscription: (meetingId) =>
      transport.invoke('retry_transcription', { request: { meetingId } }),
    retryMinutes: (meetingId, transcriptRevision) =>
      transport.invoke('retry_minutes', {
        request: { meetingId, transcriptRevision },
      }),
    subscribe: (listener) =>
      transport.listen<ProcessingJobEvent>('processing-job-event', listener),
  }
}

export const providerClient = createProviderClient()
export const processingJobClient = createProcessingJobClient()
