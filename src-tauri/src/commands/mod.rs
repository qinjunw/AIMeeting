pub mod jobs;
mod meetings;
pub mod providers;
mod recording;

use chrono::Utc;
use jobs::{JobState, ProcessingJobStatus, ProcessingJobStatusRequest, RetryJobRequest};
use tauri::{AppHandle, State};

pub(crate) use meetings::{
    get_meeting, get_meeting_detail, list_meetings, list_trash, permanently_delete_meetings,
    rename_meeting, restore_meetings, trash_meetings,
};
pub(crate) use providers::{
    delete_provider_profile, list_provider_profiles, save_provider_profile, test_provider_profile,
};
pub(crate) use recording::{
    get_active_meeting, list_audio_devices, pause_recording, resume_recording, start_recording,
    stop_recording,
};

#[tauri::command]
pub(crate) fn list_processing_jobs(
    state: State<'_, JobState>,
    request: ProcessingJobStatusRequest,
) -> Result<Vec<ProcessingJobStatus>, String> {
    let store = state.store().map_err(|error| error.to_string())?;
    jobs::processing_job_status(&store, request).map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn retry_transcription(
    app: AppHandle,
    state: State<'_, JobState>,
    desktop: State<'_, crate::runtime::DesktopState>,
    request: jobs::DesktopRetryJobRequest,
) -> Result<ProcessingJobStatus, String> {
    let mut store = state.store().map_err(|error| error.to_string())?;
    let revision = desktop
        .repository
        .lock()
        .map_err(|error| format!("应用内部状态锁不可用：{error}"))?
        .latest_transcript_revision(&request.meeting_id)
        .map_err(|error| error.to_string())?
        + 1;
    let status = jobs::retry_or_enqueue(
        &mut store,
        crate::jobs::JobKind::FileTranscription,
        RetryJobRequest {
            meeting_id: request.meeting_id,
            input_revision: Some(
                u64::try_from(revision).map_err(|_| "转写版本号无效。".to_string())?,
            ),
            requested_at: Utc::now().to_rfc3339(),
        },
    )
    .map_err(|error| error.to_string())?;
    crate::runtime::processing::spawn(app);
    Ok(status)
}

#[tauri::command]
pub(crate) fn retry_minutes(
    app: AppHandle,
    state: State<'_, JobState>,
    desktop: State<'_, crate::runtime::DesktopState>,
    request: jobs::DesktopRetryJobRequest,
) -> Result<ProcessingJobStatus, String> {
    let mut store = state.store().map_err(|error| error.to_string())?;
    let revision = match request.transcript_revision {
        Some(revision) => revision,
        None => u64::try_from(
            desktop
                .repository
                .lock()
                .map_err(|error| format!("应用内部状态锁不可用：{error}"))?
                .latest_transcript_revision(&request.meeting_id)
                .map_err(|error| error.to_string())?,
        )
        .map_err(|_| "转写版本号无效。".to_string())?,
    };
    let status = jobs::retry_or_enqueue(
        &mut store,
        crate::jobs::JobKind::Minutes,
        RetryJobRequest {
            meeting_id: request.meeting_id,
            input_revision: Some(revision),
            requested_at: Utc::now().to_rfc3339(),
        },
    )
    .map_err(|error| error.to_string())?;
    crate::runtime::processing::spawn(app);
    Ok(status)
}

use crate::gateways::{
    file_asr::openai_compatible::{self, TranscribeAudioRequest, TranscribeAudioResponse},
    live_asr::dashscope::{
        self, PushStreamingAsrAudioRequest, StartStreamingAsrRequest, StartStreamingAsrResponse,
        StreamingAsrSessionRequest, StreamingAsrState,
    },
};
use serde::Serialize;
#[derive(Serialize)]
pub(crate) struct CaptureCapabilities {
    platform: String,
    system_audio: String,
    microphone: String,
    note: String,
}

#[tauri::command]
pub(crate) fn capture_capabilities() -> CaptureCapabilities {
    CaptureCapabilities {
        platform: std::env::consts::OS.to_string(),
        system_audio: "windows-wasapi-loopback-planned".to_string(),
        microphone: "cpal-or-wasapi-planned".to_string(),
        note: "The React prototype probes browser media. Native capture belongs behind this command boundary.".to_string(),
    }
}

#[tauri::command]
pub(crate) fn secure_key_storage_status() -> String {
    "planned: use Windows Credential Manager or a Tauri keyring plugin before persisting provider keys".to_string()
}

#[tauri::command]
pub(crate) async fn transcribe_audio_chunk(
    request: TranscribeAudioRequest,
) -> Result<TranscribeAudioResponse, String> {
    openai_compatible::transcribe_audio_chunk(request).await
}

#[tauri::command]
pub(crate) async fn start_streaming_asr_session(
    app: AppHandle,
    state: tauri::State<'_, StreamingAsrState>,
    request: StartStreamingAsrRequest,
) -> Result<StartStreamingAsrResponse, String> {
    dashscope::start_streaming_asr_session(app, state.inner(), request).await
}

#[tauri::command]
pub(crate) async fn push_streaming_asr_audio(
    state: tauri::State<'_, StreamingAsrState>,
    request: PushStreamingAsrAudioRequest,
) -> Result<(), String> {
    dashscope::push_streaming_asr_audio(state.inner(), request).await
}

#[tauri::command]
pub(crate) async fn finish_streaming_asr_session(
    state: tauri::State<'_, StreamingAsrState>,
    request: StreamingAsrSessionRequest,
) -> Result<(), String> {
    dashscope::finish_streaming_asr_session(state.inner(), request).await
}

#[tauri::command]
pub(crate) async fn cancel_streaming_asr_session(
    state: tauri::State<'_, StreamingAsrState>,
    request: StreamingAsrSessionRequest,
) -> Result<(), String> {
    dashscope::cancel_streaming_asr_session(state.inner(), request).await
}
