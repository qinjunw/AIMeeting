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
use tauri::{LogicalPosition, LogicalSize, Manager};

const WINDOW_WIDTH_RATIO: f64 = 0.88;
const WINDOW_HEIGHT_RATIO: f64 = 0.84;
const MIN_WINDOW_WIDTH: f64 = 760.0;
const MIN_WINDOW_HEIGHT: f64 = 500.0;
const MAX_WINDOW_WIDTH: f64 = 1320.0;
const MAX_WINDOW_HEIGHT: f64 = 820.0;

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

            if let Some(window) = app.get_webview_window("main") {
                if let Some(monitor) = window.primary_monitor()? {
                    let work_area = monitor.work_area();
                    let scale_factor = normalized_scale_factor(monitor.scale_factor());
                    let size = initial_window_size(
                        work_area.size.width,
                        work_area.size.height,
                        scale_factor,
                    );
                    let work_width = work_area.size.width as f64 / scale_factor;
                    let work_height = work_area.size.height as f64 / scale_factor;
                    let work_x = work_area.position.x as f64 / scale_factor;
                    let work_y = work_area.position.y as f64 / scale_factor;
                    let position = LogicalPosition::new(
                        (work_x + (work_width - size.width) / 2.0).round(),
                        (work_y + (work_height - size.height) / 2.0).round(),
                    );

                    window.set_size(size)?;
                    window.set_position(position)?;
                }
                window.show()?;
            }
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

fn initial_window_size(
    physical_work_width: u32,
    physical_work_height: u32,
    scale_factor: f64,
) -> LogicalSize<f64> {
    let scale_factor = normalized_scale_factor(scale_factor);
    let logical_width = physical_work_width as f64 / scale_factor;
    let logical_height = physical_work_height as f64 / scale_factor;

    LogicalSize::new(
        (logical_width * WINDOW_WIDTH_RATIO)
            .round()
            .clamp(MIN_WINDOW_WIDTH, MAX_WINDOW_WIDTH),
        (logical_height * WINDOW_HEIGHT_RATIO)
            .round()
            .clamp(MIN_WINDOW_HEIGHT, MAX_WINDOW_HEIGHT),
    )
}

fn normalized_scale_factor(scale_factor: f64) -> f64 {
    if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    }
}

#[cfg(test)]
mod window_size_tests {
    use super::*;

    #[test]
    fn laptop_window_keeps_margin_around_the_work_area() {
        let size = initial_window_size(1366, 728, 1.0);

        assert_eq!(size.width, 1202.0);
        assert_eq!(size.height, 612.0);
    }

    #[test]
    fn high_dpi_monitor_uses_logical_pixels_and_respects_the_cap() {
        let size = initial_window_size(2560, 1560, 1.5);

        assert_eq!(size.width, 1320.0);
        assert_eq!(size.height, 820.0);
    }

    #[test]
    fn compact_work_area_keeps_the_interface_operable() {
        let size = initial_window_size(900, 560, 1.0);

        assert_eq!(size.width, 792.0);
        assert_eq!(size.height, 500.0);
    }
}
