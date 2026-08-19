pub mod audio;
pub mod commands;
pub mod domain;
pub mod gateways;
pub mod jobs;
pub mod meeting;
pub mod persistence;
pub mod runtime;

use commands::{
    cancel_streaming_asr_session, capture_capabilities, delete_provider_profile,
    finish_streaming_asr_session, get_active_meeting, get_meeting, get_meeting_detail,
    list_audio_devices, list_meetings, list_processing_jobs, list_provider_profiles, list_trash,
    pause_recording, permanently_delete_meetings, push_streaming_asr_audio, rename_meeting,
    restore_meetings, resume_recording, retry_minutes, retry_transcription, save_provider_profile,
    secure_key_storage_status, start_recording, start_streaming_asr_session, stop_recording,
    test_provider_profile, transcribe_audio_chunk, trash_meetings,
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
            let desktop = DesktopState::open(root)?;
            app.manage(commands::providers::ProviderState::new(
                desktop.paths.database_path().to_path_buf(),
            ));
            app.manage(commands::jobs::JobState::new(
                desktop.paths.database_path().to_path_buf(),
            ));
            app.manage(desktop);
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
            list_provider_profiles,
            save_provider_profile,
            delete_provider_profile,
            test_provider_profile,
            list_processing_jobs,
            retry_transcription,
            retry_minutes,
            transcribe_audio_chunk,
            start_streaming_asr_session,
            push_streaming_asr_audio,
            finish_streaming_asr_session,
            cancel_streaming_asr_session
        ])
        .run(tauri::generate_context!())
        .expect("error while running AIMeeting");
}
