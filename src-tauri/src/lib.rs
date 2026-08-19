pub mod audio;
pub mod commands;
pub mod domain;
pub mod gateways;
pub mod jobs;
pub mod meeting;
pub mod persistence;
pub mod runtime;

use commands::{
    delete_provider_profile, get_active_meeting, get_meeting, get_meeting_detail,
    import_legacy_meetings, list_audio_devices, list_meetings, list_processing_jobs,
    list_provider_profiles, list_trash, pause_recording, permanently_delete_meetings,
    rename_meeting, restore_meetings, resume_recording, retry_minutes, retry_transcription,
    save_provider_profile, start_recording, stop_recording, test_provider_profile, trash_meetings,
};
use gateways::live_asr::dashscope::StreamingAsrState;
use runtime::DesktopState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(StreamingAsrState::default())
        .setup(|app| {
            let local_root = app.path().app_local_data_dir()?;
            let legacy_root = app.path().app_data_dir()?;
            crate::persistence::migrate_legacy_data_root(&legacy_root, &local_root)?;
            let desktop = DesktopState::open(local_root)?;
            app.manage(commands::providers::ProviderState::new(
                desktop.paths.database_path().to_path_buf(),
            ));
            app.manage(commands::jobs::JobState::new(
                desktop.paths.database_path().to_path_buf(),
            ));
            app.manage(desktop);
            runtime::transcription::install(app.handle());
            runtime::processing::recover_and_spawn(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_audio_devices,
            start_recording,
            pause_recording,
            resume_recording,
            stop_recording,
            get_active_meeting,
            import_legacy_meetings,
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
            retry_minutes
        ])
        .run(tauri::generate_context!())
        .expect("error while running AIMeeting");
}
