use chrono::Utc;
use serde::Deserialize;
use serde_json::json;
use tauri::{AppHandle, Emitter, Listener, Manager};

use crate::jobs::{JobKind, NewPersistentJob, SqliteJobStore};
use crate::runtime::DesktopState;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StreamingAsrEvent {
    #[serde(rename = "sessionId")]
    _session_id: String,
    meeting_id: String,
    #[serde(rename = "recordingRunId")]
    _recording_run_id: String,
    status: StreamingAsrStatus,
    text: String,
    begin_ms: Option<i64>,
    end_ms: Option<i64>,
    #[serde(rename = "providerLabel")]
    _provider_label: String,
    error_message: Option<String>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum StreamingAsrStatus {
    Started,
    Interim,
    Final,
    Finished,
    Error,
}

pub fn install(app: &AppHandle) {
    let handle = app.clone();
    app.listen("streaming-asr-event", move |event| {
        let Ok(event) = serde_json::from_str::<StreamingAsrEvent>(event.payload()) else {
            return;
        };
        process_event(&handle, event);
    });
}

fn process_event(app: &AppHandle, event: StreamingAsrEvent) {
    let state = app.state::<DesktopState>();
    let now = Utc::now().to_rfc3339();
    let Ok(repository) = state.repository.lock() else {
        return;
    };
    let Ok(run_generation) = repository.latest_recording_generation(&event.meeting_id) else {
        return;
    };
    let Ok(latest_revision) = repository.latest_transcript_revision(&event.meeting_id) else {
        return;
    };
    let next_revision = latest_revision + 1;

    match event.status {
        StreamingAsrStatus::Started => {
            let _ = repository.update_transcription_status(&event.meeting_id, "streaming", &now);
            emit_status(
                app,
                &event.meeting_id,
                run_generation,
                latest_revision,
                "streaming",
                None,
            );
        }
        StreamingAsrStatus::Interim => {
            let _ = app.emit(
                "transcription-event",
                json!({
                    "event": "interim",
                    "meetingId": event.meeting_id,
                    "runGeneration": run_generation,
                    "revision": next_revision,
                    "text": event.text,
                }),
            );
        }
        StreamingAsrStatus::Final => {
            let segment_id = uuid::Uuid::new_v4().to_string();
            if repository
                .append_transcript_segment(
                    &segment_id,
                    &event.meeting_id,
                    next_revision,
                    event.begin_ms.unwrap_or_default().max(0),
                    event
                        .end_ms
                        .unwrap_or_else(|| event.begin_ms.unwrap_or_default())
                        .max(0),
                    event.text.trim(),
                )
                .is_ok()
            {
                let _ =
                    repository.update_transcription_status(&event.meeting_id, "streaming", &now);
                let _ = app.emit(
                    "transcription-event",
                    json!({
                        "event": "final",
                        "meetingId": event.meeting_id,
                        "runGeneration": run_generation,
                        "revision": next_revision,
                        "segmentId": segment_id,
                        "text": event.text,
                        "beginMs": event.begin_ms,
                        "endMs": event.end_ms,
                    }),
                );
            }
        }
        StreamingAsrStatus::Finished => {
            let final_revision = repository
                .latest_transcript_revision(&event.meeting_id)
                .unwrap_or(latest_revision);
            let _ = repository.update_transcription_status(&event.meeting_id, "ready", &now);
            emit_status(
                app,
                &event.meeting_id,
                run_generation,
                final_revision,
                "ready",
                None,
            );
            drop(repository);
            if final_revision > 0 {
                enqueue_minutes(app, &event.meeting_id, final_revision as u64, &now);
            }
        }
        StreamingAsrStatus::Error => {
            let message = event
                .error_message
                .unwrap_or_else(|| "实时转写暂不可用，录音仍在保存。".to_string());
            let _ = repository.update_transcription_status(&event.meeting_id, "failed", &now);
            emit_status(
                app,
                &event.meeting_id,
                run_generation,
                latest_revision,
                "failed",
                Some(message),
            );
            drop(repository);
            crate::runtime::processing::enqueue_file_transcription_if_recording_complete(
                app,
                &event.meeting_id,
            );
        }
    }
}

fn emit_status(
    app: &AppHandle,
    meeting_id: &str,
    run_generation: i64,
    revision: i64,
    status: &str,
    error: Option<String>,
) {
    let _ = app.emit(
        "transcription-event",
        json!({
            "event": "status",
            "meetingId": meeting_id,
            "runGeneration": run_generation.max(0) as u64,
            "revision": revision.max(0) as u64,
            "status": status,
            "error": error,
        }),
    );
}

fn enqueue_minutes(app: &AppHandle, meeting_id: &str, revision: u64, now: &str) {
    let state = app.state::<DesktopState>();
    let Ok(mut store) = SqliteJobStore::open(state.paths.database_path()) else {
        return;
    };
    let _ = store.enqueue(NewPersistentJob::new(
        uuid::Uuid::new_v4().to_string(),
        meeting_id,
        JobKind::Minutes,
        Some(revision),
        now,
    ));
    crate::runtime::processing::spawn(app.clone());
}
