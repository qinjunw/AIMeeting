use chrono::Utc;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::audio::capture::{AudioDeviceInfo, SourceSelection};
use crate::audio::platform::windows::enumerate_audio_devices;
use crate::persistence::NewMeetingRecord;
use crate::runtime::registry::{recording_file_size, ActiveRecordingSnapshot, RecordingCheckpoint};
use crate::runtime::DesktopState;

const RECORDING_FILE_NAME: &str = "recording.opus";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StartRecordingRequest {
    title: Option<String>,
    sources: AudioSourcesRequest,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AudioSourcesRequest {
    microphone: bool,
    system_audio: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MeetingRecordingRequest {
    meeting_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResumeRecordingRequest {
    meeting_id: String,
    sources: AudioSourcesRequest,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionSnapshot {
    meeting_id: String,
    run_generation: u64,
    recording_status: String,
    transcript_revision: u64,
    transcription_status: String,
    transcription_error: Option<String>,
    minutes_status: String,
    minutes_error: Option<String>,
}

#[tauri::command]
pub(crate) fn list_audio_devices() -> Result<Vec<AudioDeviceInfo>, String> {
    enumerate_audio_devices().map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn start_recording(
    app: AppHandle,
    state: State<'_, DesktopState>,
    request: StartRecordingRequest,
) -> Result<SessionSnapshot, String> {
    let selection = source_selection(request.sources.microphone, request.sources.system_audio)?;
    let meeting_id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let title = request
        .title
        .map(|title| title.trim().to_string())
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| "新会议".to_string());
    let meeting = NewMeetingRecord {
        id: meeting_id.clone(),
        title,
        status: "preparing".to_string(),
        transcription_status: "pending".to_string(),
        minutes_status: "pending".to_string(),
        created_at: now.clone(),
    };
    state
        .repository
        .lock()
        .map_err(lock_error)?
        .create_meeting(&meeting)
        .map_err(|error| error.to_string())?;

    let meeting_dir = state
        .paths
        .meeting_dir(&meeting_id)
        .map_err(|error| error.to_string())?;
    let audio_path = meeting_dir.join(RECORDING_FILE_NAME);
    let start_result = state.recordings.lock().map_err(lock_error)?.start(
        meeting_id.clone(),
        1,
        selection,
        audio_path,
    );
    if let Err(error) = start_result {
        let _ = state
            .repository
            .lock()
            .map_err(lock_error)?
            .update_meeting_states(
                &meeting_id,
                "interrupted",
                "pending",
                "pending",
                &Utc::now().to_rfc3339(),
            );
        return Err(error);
    }

    let persist_result = (|| {
        let repository = state.repository.lock().map_err(lock_error)?;
        repository
            .create_recording_run(
                &run_id(&meeting_id, 1),
                &meeting_id,
                1,
                1,
                &sources_json(selection),
                &now,
            )
            .map_err(|error| error.to_string())?;
        repository
            .upsert_recording_asset(&meeting_id, RECORDING_FILE_NAME, "recording", 0, 0, &now)
            .map_err(|error| error.to_string())?;
        repository
            .update_meeting_states(&meeting_id, "recording", "pending", "pending", &now)
            .map_err(|error| error.to_string())
    })();
    if let Err(error) = persist_result {
        let _ = state
            .recordings
            .lock()
            .map_err(lock_error)?
            .stop(&meeting_id);
        return Err(error);
    }

    load_and_emit(&app, &state, &meeting_id, 1)
}

#[tauri::command]
pub(crate) fn pause_recording(
    app: AppHandle,
    state: State<'_, DesktopState>,
    request: MeetingRecordingRequest,
) -> Result<SessionSnapshot, String> {
    let snapshot = state
        .recordings
        .lock()
        .map_err(lock_error)?
        .pause(&request.meeting_id)?;
    let now = Utc::now().to_rfc3339();
    let repository = state.repository.lock().map_err(lock_error)?;
    repository
        .finish_recording_run(&run_id(&request.meeting_id, snapshot.generation), &now)
        .map_err(|error| error.to_string())?;
    repository
        .update_meeting_states(&request.meeting_id, "paused", "pending", "pending", &now)
        .map_err(|error| error.to_string())?;
    drop(repository);
    load_and_emit(&app, &state, &request.meeting_id, snapshot.generation)
}

#[tauri::command]
pub(crate) fn resume_recording(
    app: AppHandle,
    state: State<'_, DesktopState>,
    request: ResumeRecordingRequest,
) -> Result<SessionSnapshot, String> {
    let selection = source_selection(request.sources.microphone, request.sources.system_audio)?;
    let current = state
        .recordings
        .lock()
        .map_err(lock_error)?
        .active()
        .ok_or_else(|| "当前没有可继续的会议。".to_string())?;
    if current.meeting_id != request.meeting_id {
        return Err("请求继续的会议不是当前活动会议。".to_string());
    }
    let generation = current.generation + 1;
    state.recordings.lock().map_err(lock_error)?.resume(
        &request.meeting_id,
        generation,
        selection,
    )?;

    let now = Utc::now().to_rfc3339();
    let repository = state.repository.lock().map_err(lock_error)?;
    repository
        .create_recording_run(
            &run_id(&request.meeting_id, generation),
            &request.meeting_id,
            i64::try_from(generation).map_err(|_| "录音分段序号过大。".to_string())?,
            i64::try_from(generation).map_err(|_| "录音代次过大。".to_string())?,
            &sources_json(selection),
            &now,
        )
        .map_err(|error| error.to_string())?;
    repository
        .update_meeting_states(&request.meeting_id, "recording", "pending", "pending", &now)
        .map_err(|error| error.to_string())?;
    drop(repository);
    load_and_emit(&app, &state, &request.meeting_id, generation)
}

#[tauri::command]
pub(crate) fn stop_recording(
    app: AppHandle,
    state: State<'_, DesktopState>,
    request: MeetingRecordingRequest,
) -> Result<SessionSnapshot, String> {
    let snapshot = state
        .recordings
        .lock()
        .map_err(lock_error)?
        .active()
        .ok_or_else(|| "当前没有正在录音的会议。".to_string())?;
    if snapshot.meeting_id != request.meeting_id {
        return Err("请求结束的会议不是当前活动会议。".to_string());
    }
    let checkpoint = state
        .recordings
        .lock()
        .map_err(lock_error)?
        .stop(&request.meeting_id)?;
    persist_stopped_recording(&state, &snapshot, checkpoint)?;
    load_and_emit(&app, &state, &request.meeting_id, snapshot.generation)
}

#[tauri::command]
pub(crate) fn get_active_meeting(
    state: State<'_, DesktopState>,
) -> Result<Option<SessionSnapshot>, String> {
    let active = state.recordings.lock().map_err(lock_error)?.active();
    active
        .map(|active| session_snapshot(&state, &active.meeting_id, active.generation))
        .transpose()
}

fn persist_stopped_recording(
    state: &DesktopState,
    snapshot: &ActiveRecordingSnapshot,
    checkpoint: RecordingCheckpoint,
) -> Result<(), String> {
    let now = Utc::now().to_rfc3339();
    let duration_ms = i64::try_from(checkpoint.recorded_samples / 48)
        .map_err(|_| "录音时长超出支持范围。".to_string())?;
    let byte_size = i64::try_from(recording_file_size(&snapshot.audio_path)?)
        .map_err(|_| "录音文件大小超出支持范围。".to_string())?;
    let repository = state.repository.lock().map_err(lock_error)?;
    repository
        .finish_recording_run(&run_id(&snapshot.meeting_id, snapshot.generation), &now)
        .map_err(|error| error.to_string())?;
    repository
        .upsert_recording_asset(
            &snapshot.meeting_id,
            RECORDING_FILE_NAME,
            "ready",
            duration_ms,
            byte_size,
            &now,
        )
        .map_err(|error| error.to_string())?;
    repository
        .mark_meeting_stopped(&snapshot.meeting_id, "ready", "pending", "pending", &now)
        .map_err(|error| error.to_string())
}

fn load_and_emit(
    app: &AppHandle,
    state: &DesktopState,
    meeting_id: &str,
    run_generation: u64,
) -> Result<SessionSnapshot, String> {
    let snapshot = session_snapshot(state, meeting_id, run_generation)?;
    app.emit("meeting-state-event", snapshot.clone())
        .map_err(|error| error.to_string())?;
    Ok(snapshot)
}

fn session_snapshot(
    state: &DesktopState,
    meeting_id: &str,
    run_generation: u64,
) -> Result<SessionSnapshot, String> {
    let repository = state.repository.lock().map_err(lock_error)?;
    let meeting = repository
        .get_meeting(meeting_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "会议记录不存在。".to_string())?;
    let transcript_revision = repository
        .latest_transcript_revision(meeting_id)
        .map_err(|error| error.to_string())?;
    Ok(SessionSnapshot {
        meeting_id: meeting.id,
        run_generation,
        recording_status: meeting.status,
        transcript_revision: u64::try_from(transcript_revision)
            .map_err(|_| "转写版本号无效。".to_string())?,
        transcription_status: meeting.transcription_status,
        transcription_error: None,
        minutes_status: meeting.minutes_status,
        minutes_error: None,
    })
}

fn source_selection(microphone: bool, system: bool) -> Result<SourceSelection, String> {
    if !microphone && !system {
        return Err("请至少选择麦克风或系统声音中的一路。".to_string());
    }
    Ok(SourceSelection { microphone, system })
}

fn sources_json(selection: SourceSelection) -> String {
    serde_json::json!({
        "microphone": selection.microphone,
        "system": selection.system,
        "mixBeforeTranscription": true
    })
    .to_string()
}

fn run_id(meeting_id: &str, generation: u64) -> String {
    format!("{meeting_id}-run-{generation}")
}

fn lock_error<T>(error: std::sync::PoisonError<T>) -> String {
    format!("应用内部状态锁不可用：{error}")
}
