mod meetings;
mod recording;

pub(crate) use meetings::{
    get_meeting, get_meeting_detail, list_meetings, list_trash, permanently_delete_meetings,
    rename_meeting, restore_meetings, trash_meetings,
};
pub(crate) use recording::{
    get_active_meeting, list_audio_devices, pause_recording, resume_recording, start_recording,
    stop_recording,
};

use crate::gateways::{
    file_asr::openai_compatible::{self, TranscribeAudioRequest, TranscribeAudioResponse},
    live_asr::dashscope::{
        self, PushStreamingAsrAudioRequest, StartStreamingAsrRequest, StartStreamingAsrResponse,
        StreamingAsrSessionRequest, StreamingAsrState,
    },
};
use serde::Serialize;
use tauri::AppHandle;

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
