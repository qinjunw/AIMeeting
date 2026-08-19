mod commands;
mod gateways;

use commands::{
    cancel_streaming_asr_session, capture_capabilities, finish_streaming_asr_session,
    push_streaming_asr_audio, secure_key_storage_status, start_streaming_asr_session,
    transcribe_audio_chunk,
};
use gateways::live_asr::dashscope::StreamingAsrState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(StreamingAsrState::default())
        .invoke_handler(tauri::generate_handler![
            capture_capabilities,
            secure_key_storage_status,
            transcribe_audio_chunk,
            start_streaming_asr_session,
            push_streaming_asr_audio,
            finish_streaming_asr_session,
            cancel_streaming_asr_session
        ])
        .run(tauri::generate_context!())
        .expect("error while running AIMeeting");
}
