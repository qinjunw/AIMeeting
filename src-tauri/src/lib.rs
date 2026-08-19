pub mod audio;
mod commands;
pub mod domain;
pub mod gateways;
pub mod meeting;
pub mod persistence;
pub mod runtime;

use commands::{
    cancel_streaming_asr_session, capture_capabilities, finish_streaming_asr_session,
    get_active_meeting, get_meeting, get_meeting_detail, list_audio_devices, list_meetings,
    list_trash, pause_recording, permanently_delete_meetings, push_streaming_asr_audio,
    rename_meeting, restore_meetings, resume_recording, secure_key_storage_status, start_recording,
    start_streaming_asr_session, stop_recording, transcribe_audio_chunk, trash_meetings,
};
use gateways::live_asr::dashscope::StreamingAsrState;
use runtime::DesktopState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(StreamingAsrState::default())
        .setup(|app| {
            let root = app.path().app_data_dir()?;
            app.manage(DesktopState::open(root)?);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            capture_capabilities,
            secure_key_storage_status,
            list_audio_devices,
            start_recording,
            pause_recording,
            resume_recording,
            stop_recording,
            get_active_meeting,
            list_meetings,
            get_meeting,
            get_meeting_detail,
            rename_meeting,
            list_trash,
            trash_meetings,
            restore_meetings,
            permanently_delete_meetings,
            transcribe_audio_chunk,
            start_streaming_asr_session,
            push_streaming_asr_audio,
            finish_streaming_asr_session,
            cancel_streaming_asr_session
        ])
        .run(tauri::generate_context!())
        .expect("error while running AIMeeting");
}
