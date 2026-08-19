use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::jobs::{JobError, JobKind, PersistentJob, SqliteJobStore};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RetryJobRequest {
    pub meeting_id: String,
    pub input_revision: Option<u64>,
    pub requested_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProcessingJobStatusRequest {
    pub meeting_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopRetryJobRequest {
    pub meeting_id: String,
    pub transcript_revision: Option<u64>,
}

pub struct JobState {
    database_path: PathBuf,
}

impl JobState {
    pub fn new(database_path: PathBuf) -> Self {
        Self { database_path }
    }

    pub fn store(&self) -> Result<SqliteJobStore, JobError> {
        SqliteJobStore::open(&self.database_path)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessingJobStatus {
    pub id: String,
    pub meeting_id: String,
    pub kind: JobKind,
    pub status: crate::jobs::JobStatus,
    pub attempts: u32,
    pub input_revision: Option<u64>,
    pub error_summary: Option<String>,
}

impl From<PersistentJob> for ProcessingJobStatus {
    fn from(job: PersistentJob) -> Self {
        Self {
            id: job.id,
            meeting_id: job.meeting_id,
            kind: job.kind,
            status: job.status,
            attempts: job.attempts,
            input_revision: job.input_revision,
            error_summary: job.error_summary,
        }
    }
}

pub fn retry_transcription(
    store: &mut SqliteJobStore,
    request: RetryJobRequest,
) -> Result<ProcessingJobStatus, JobError> {
    retry(store, JobKind::FileTranscription, request)
}

pub fn retry_minutes(
    store: &mut SqliteJobStore,
    request: RetryJobRequest,
) -> Result<ProcessingJobStatus, JobError> {
    retry(store, JobKind::Minutes, request)
}

pub fn processing_job_status(
    store: &SqliteJobStore,
    request: ProcessingJobStatusRequest,
) -> Result<Vec<ProcessingJobStatus>, JobError> {
    store
        .list_for_meeting(&request.meeting_id)
        .map(|jobs| jobs.into_iter().map(Into::into).collect())
}

fn retry(
    store: &mut SqliteJobStore,
    kind: JobKind,
    request: RetryJobRequest,
) -> Result<ProcessingJobStatus, JobError> {
    store
        .retry_latest_failed(
            &request.meeting_id,
            kind,
            request.input_revision,
            &request.requested_at,
        )
        .map(Into::into)
}
