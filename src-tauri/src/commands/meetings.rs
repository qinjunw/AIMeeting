use chrono::Utc;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::persistence::{MeetingMinutesRow, MeetingRecordRow, RecordingAssetRow};
use crate::runtime::DesktopState;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GetMeetingRequest {
    meeting_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RenameMeetingRequest {
    meeting_id: String,
    title: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MeetingIdsRequest {
    meeting_ids: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MeetingDetail {
    meeting: MeetingRecordRow,
    transcript_revision: i64,
    transcript: String,
    minutes: Option<MeetingMinutesRow>,
    recording: Option<RecordingAssetRow>,
}

#[tauri::command]
pub(crate) fn list_meetings(
    state: State<'_, DesktopState>,
) -> Result<Vec<MeetingRecordRow>, String> {
    state
        .repository
        .lock()
        .map_err(lock_error)?
        .list_meetings(false)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn get_meeting(
    state: State<'_, DesktopState>,
    request: GetMeetingRequest,
) -> Result<Option<MeetingRecordRow>, String> {
    state
        .repository
        .lock()
        .map_err(lock_error)?
        .get_meeting(&request.meeting_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn get_meeting_detail(
    state: State<'_, DesktopState>,
    request: GetMeetingRequest,
) -> Result<Option<MeetingDetail>, String> {
    let repository = state.repository.lock().map_err(lock_error)?;
    let Some(meeting) = repository
        .get_meeting(&request.meeting_id)
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    Ok(Some(MeetingDetail {
        transcript_revision: repository
            .latest_transcript_revision(&request.meeting_id)
            .map_err(|error| error.to_string())?,
        transcript: repository
            .full_transcript(&request.meeting_id)
            .map_err(|error| error.to_string())?,
        minutes: repository
            .latest_minutes(&request.meeting_id)
            .map_err(|error| error.to_string())?,
        recording: repository
            .latest_recording_asset(&request.meeting_id)
            .map_err(|error| error.to_string())?,
        meeting,
    }))
}

#[tauri::command]
pub(crate) fn rename_meeting(
    state: State<'_, DesktopState>,
    request: RenameMeetingRequest,
) -> Result<(), String> {
    let title = request.title.trim();
    if title.is_empty() {
        return Err("会议名称不能为空。".to_string());
    }
    state
        .repository
        .lock()
        .map_err(lock_error)?
        .rename_meeting(&request.meeting_id, title, &Utc::now().to_rfc3339())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn list_trash(state: State<'_, DesktopState>) -> Result<Vec<MeetingRecordRow>, String> {
    state
        .repository
        .lock()
        .map_err(lock_error)?
        .list_meetings(true)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn trash_meetings(
    state: State<'_, DesktopState>,
    request: MeetingIdsRequest,
) -> Result<(), String> {
    reject_active_meetings(&state, &request.meeting_ids)?;
    for meeting_id in normalized_ids(request.meeting_ids) {
        let now = Utc::now().to_rfc3339();
        state
            .repository
            .lock()
            .map_err(lock_error)?
            .soft_delete_meeting(&meeting_id, &now)
            .map_err(|error| error.to_string())?;
        let source = state
            .paths
            .meeting_dir(&meeting_id)
            .map_err(|error| error.to_string())?;
        if source.exists() {
            if let Err(error) = state.paths.move_to_trash(&meeting_id) {
                let _ = state
                    .repository
                    .lock()
                    .map_err(lock_error)?
                    .restore_meeting(&meeting_id);
                return Err(error.to_string());
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn restore_meetings(
    state: State<'_, DesktopState>,
    request: MeetingIdsRequest,
) -> Result<(), String> {
    for meeting_id in normalized_ids(request.meeting_ids) {
        state
            .repository
            .lock()
            .map_err(lock_error)?
            .restore_meeting(&meeting_id)
            .map_err(|error| error.to_string())?;
        let source = state
            .paths
            .trash_dir(&meeting_id)
            .map_err(|error| error.to_string())?;
        if source.exists() {
            if let Err(error) = state.paths.restore_from_trash(&meeting_id) {
                let _ = state
                    .repository
                    .lock()
                    .map_err(lock_error)?
                    .soft_delete_meeting(&meeting_id, &Utc::now().to_rfc3339());
                return Err(error.to_string());
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn permanently_delete_meetings(
    state: State<'_, DesktopState>,
    request: MeetingIdsRequest,
) -> Result<(), String> {
    reject_active_meetings(&state, &request.meeting_ids)?;
    for meeting_id in normalized_ids(request.meeting_ids) {
        state
            .paths
            .permanently_delete_from_trash(&meeting_id)
            .map_err(|error| error.to_string())?;
        state
            .repository
            .lock()
            .map_err(lock_error)?
            .permanently_delete_meeting(&meeting_id)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn reject_active_meetings(state: &DesktopState, meeting_ids: &[String]) -> Result<(), String> {
    let active = state.recordings.lock().map_err(lock_error)?.active();
    if active.is_some_and(|active| meeting_ids.iter().any(|id| id == &active.meeting_id)) {
        return Err("正在录音的会议不能移动到回收站或永久删除。".to_string());
    }
    Ok(())
}

fn normalized_ids(meeting_ids: Vec<String>) -> Vec<String> {
    let mut ids = meeting_ids
        .into_iter()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect::<Vec<_>>();
    ids.sort();
    ids.dedup();
    ids
}

fn lock_error<T>(error: std::sync::PoisonError<T>) -> String {
    format!("应用内部状态锁不可用：{error}")
}
