use std::time::Duration;

use chrono::Utc;
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};

use crate::commands::jobs::{retry_or_enqueue, RetryJobRequest};
use crate::commands::providers::{EndpointFlavor, ProviderState};
use crate::domain::ProviderCapability;
use crate::gateways::file_asr::openai_compatible::{
    transcribe_audio_file, FileTranscriptionConfig,
};
use crate::gateways::minutes::openai_compatible::{
    CancellationToken, MinutesInput, OpenAiCompatibleConfig, OpenAiCompatibleMinutesClient,
    TextEndpointFlavor,
};
use crate::jobs::{
    JobKind, JobOutput, JobRunDisposition, JobStatus, NewPersistentJob, PersistentJob,
    SqliteJobStore,
};
use crate::runtime::DesktopState;

const MAX_JOB_ATTEMPTS: u32 = 3;

pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        drain_kind(&app, JobKind::FileTranscription).await;
        drain_kind(&app, JobKind::Minutes).await;
    });
}

pub fn recover_and_spawn(app: &AppHandle) {
    let state = app.state::<DesktopState>();
    if let Ok(mut store) = SqliteJobStore::open(state.paths.database_path()) {
        let _ = store.recover_interrupted(&Utc::now().to_rfc3339());
    }
    spawn(app.clone());
}

pub fn enqueue_file_transcription_if_recording_complete(app: &AppHandle, meeting_id: &str) {
    let state = app.state::<DesktopState>();
    let request = {
        let Ok(repository) = state.repository.lock() else {
            return;
        };
        let Ok(Some(meeting)) = repository.get_meeting(meeting_id) else {
            return;
        };
        if !matches!(meeting.status.as_str(), "ready" | "interrupted")
            || meeting.transcription_status != "failed"
        {
            return;
        }
        let Ok(revision) = repository.latest_transcript_revision(meeting_id) else {
            return;
        };
        let Ok(input_revision) = u64::try_from(revision.saturating_add(1)) else {
            return;
        };
        RetryJobRequest {
            meeting_id: meeting_id.to_string(),
            input_revision: Some(input_revision),
            requested_at: Utc::now().to_rfc3339(),
        }
    };

    let Ok(mut store) = SqliteJobStore::open(state.paths.database_path()) else {
        return;
    };
    if retry_or_enqueue(&mut store, JobKind::FileTranscription, request).is_ok() {
        spawn(app.clone());
    }
}

async fn drain_kind(app: &AppHandle, kind: JobKind) {
    loop {
        let now = Utc::now().to_rfc3339();
        let Some(job) = claim_next(app, kind, &now) else {
            return;
        };
        emit_job(app, &job);

        match execute(app, &job).await {
            Ok(output) => finish_success(app, &job, output),
            Err(error) => {
                if !finish_failure(app, &job, &error) {
                    return;
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
}

fn claim_next(app: &AppHandle, kind: JobKind, now: &str) -> Option<PersistentJob> {
    let state = app.state::<DesktopState>();
    SqliteJobStore::open(state.paths.database_path())
        .ok()?
        .claim_next(kind, now)
        .ok()?
}

async fn execute(app: &AppHandle, job: &PersistentJob) -> Result<JobOutput, String> {
    match job.kind {
        JobKind::FileTranscription => execute_file_transcription(app, job).await,
        JobKind::Minutes => execute_minutes(app, job).await,
    }
}

async fn execute_file_transcription(
    app: &AppHandle,
    job: &PersistentJob,
) -> Result<JobOutput, String> {
    let provider = app
        .state::<ProviderState>()
        .resolve_default(ProviderCapability::FileTranscription)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "请先在设置中配置录音文件转写 Provider。".to_string())?;
    let path = app
        .state::<DesktopState>()
        .paths
        .meeting_dir(&job.meeting_id)
        .map_err(|error| error.to_string())?
        .join("recording.opus");
    let text = transcribe_audio_file(
        &path,
        FileTranscriptionConfig {
            base_url: provider.profile.base_url,
            api_key: provider.api_key.expose().to_string(),
            model: provider.profile.model,
            language: "zh-CN".to_string(),
        },
    )
    .await?;
    Ok(JobOutput::Transcription {
        transcript_revision: required_revision(job)?,
        text,
    })
}

async fn execute_minutes(app: &AppHandle, job: &PersistentJob) -> Result<JobOutput, String> {
    let revision = required_revision(job)?;
    let provider = app
        .state::<ProviderState>()
        .resolve_default(ProviderCapability::Minutes)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "请先在设置中配置会议纪要 Provider。".to_string())?;
    let flavor = match provider.profile.endpoint_flavor {
        EndpointFlavor::ChatCompletions => TextEndpointFlavor::ChatCompletions,
        EndpointFlavor::Responses => TextEndpointFlavor::Responses,
        _ => return Err("会议纪要 Provider 的接口类型无效。".to_string()),
    };
    let (transcript, previous_minutes) = {
        let state = app.state::<DesktopState>();
        let repository = state
            .repository
            .lock()
            .map_err(|error| format!("应用内部状态锁不可用：{error}"))?;
        let transcript = repository
            .full_transcript(&job.meeting_id)
            .map_err(|error| error.to_string())?;
        let previous_minutes = repository
            .latest_minutes(&job.meeting_id)
            .map_err(|error| error.to_string())?
            .map(|minutes| minutes.content)
            .unwrap_or_default();
        (transcript, previous_minutes)
    };
    if transcript.trim().is_empty() {
        return Err("当前会议还没有可用于生成纪要的转写。".to_string());
    }
    let client = OpenAiCompatibleMinutesClient::new(OpenAiCompatibleConfig {
        base_url: provider.profile.base_url,
        model: provider.profile.model.clone(),
        api_key: provider.api_key,
        endpoint_flavor: flavor,
        timeout: Duration::from_secs(60),
    })
    .map_err(|error| error.to_string())?;
    let content = client
        .generate(
            &MinutesInput {
                meeting_id: job.meeting_id.clone(),
                transcript_revision: revision,
                previous_minutes,
                transcript,
            },
            &CancellationToken::default(),
        )
        .await
        .map_err(|error| error.to_string())?;
    Ok(JobOutput::Minutes {
        transcript_revision: revision,
        content,
        provider_label: provider.profile.model,
    })
}

fn finish_success(app: &AppHandle, job: &PersistentJob, output: JobOutput) {
    let now = Utc::now().to_rfc3339();
    let state = app.state::<DesktopState>();
    let Ok(mut store) = SqliteJobStore::open(state.paths.database_path()) else {
        return;
    };
    let event_output = output.clone();
    let Ok(disposition) = store.finish_success(job, output, &now) else {
        return;
    };
    let Ok(Some(completed)) = store.job(&job.id) else {
        return;
    };
    emit_job(app, &completed);
    if disposition != JobRunDisposition::Completed {
        return;
    }

    match event_output {
        JobOutput::Transcription {
            transcript_revision,
            text,
        } => {
            emit_transcription_result(app, job, transcript_revision, &text);
            let _ = store.enqueue(NewPersistentJob::new(
                uuid::Uuid::new_v4().to_string(),
                &job.meeting_id,
                JobKind::Minutes,
                Some(transcript_revision),
                &now,
            ));
        }
        JobOutput::Minutes {
            transcript_revision,
            content,
            ..
        } => emit_minutes_result(app, job, transcript_revision, &content, "ready", None),
    }
}

fn finish_failure(app: &AppHandle, job: &PersistentJob, error: &str) -> bool {
    let now = Utc::now().to_rfc3339();
    let state = app.state::<DesktopState>();
    let Ok(mut store) = SqliteJobStore::open(state.paths.database_path()) else {
        return false;
    };
    let Ok(status) = store.finish_failure(job, MAX_JOB_ATTEMPTS, error, &now) else {
        return false;
    };
    let error_summary = if let Ok(Some(updated)) = store.job(&job.id) {
        let summary = updated
            .error_summary
            .clone()
            .unwrap_or_else(|| "AI 服务处理失败。".to_string());
        emit_job(app, &updated);
        summary
    } else {
        "AI 服务处理失败。".to_string()
    };
    if status == JobStatus::Failed {
        match job.kind {
            JobKind::FileTranscription => {
                emit_transcription_status(app, job, "failed", Some(&error_summary))
            }
            JobKind::Minutes => emit_minutes_result(
                app,
                job,
                job.input_revision.unwrap_or_default(),
                "",
                "failed",
                Some(&error_summary),
            ),
        }
        false
    } else {
        true
    }
}

fn emit_job(app: &AppHandle, job: &PersistentJob) {
    let run_generation = latest_generation(app, &job.meeting_id);
    let _ = app.emit(
        "processing-job-event",
        json!({
            "id": job.id,
            "meetingId": job.meeting_id,
            "kind": job.kind,
            "status": job.status,
            "attempts": job.attempts,
            "inputRevision": job.input_revision,
            "errorSummary": job.error_summary,
            "runGeneration": run_generation,
            "revision": job.input_revision.unwrap_or_default(),
        }),
    );
}

fn emit_transcription_result(app: &AppHandle, job: &PersistentJob, revision: u64, text: &str) {
    let run_generation = latest_generation(app, &job.meeting_id);
    let _ = app.emit(
        "transcription-event",
        json!({
            "event": "final",
            "meetingId": job.meeting_id,
            "runGeneration": run_generation,
            "revision": revision,
            "segmentId": format!("{}-result", job.id),
            "text": text,
            "beginMs": null,
            "endMs": null,
        }),
    );
    emit_transcription_status(app, job, "ready", None);
}

fn emit_transcription_status(
    app: &AppHandle,
    job: &PersistentJob,
    status: &str,
    error: Option<&str>,
) {
    let _ = app.emit(
        "transcription-event",
        json!({
            "event": "status",
            "meetingId": job.meeting_id,
            "runGeneration": latest_generation(app, &job.meeting_id),
            "revision": job.input_revision.unwrap_or_default(),
            "status": status,
            "error": error,
        }),
    );
}

fn emit_minutes_result(
    app: &AppHandle,
    job: &PersistentJob,
    revision: u64,
    content: &str,
    status: &str,
    error: Option<&str>,
) {
    let _ = app.emit(
        "minutes-event",
        json!({
            "meetingId": job.meeting_id,
            "runGeneration": latest_generation(app, &job.meeting_id),
            "transcriptRevision": revision,
            "status": status,
            "content": if content.is_empty() { None } else { Some(content) },
            "error": error,
        }),
    );
}

fn latest_generation(app: &AppHandle, meeting_id: &str) -> u64 {
    app.state::<DesktopState>()
        .repository
        .lock()
        .ok()
        .and_then(|repository| repository.latest_recording_generation(meeting_id).ok())
        .and_then(|generation| u64::try_from(generation).ok())
        .unwrap_or_default()
}

fn required_revision(job: &PersistentJob) -> Result<u64, String> {
    job.input_revision
        .ok_or_else(|| "处理任务缺少转写版本。".to_string())
}
