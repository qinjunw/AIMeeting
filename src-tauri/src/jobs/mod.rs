mod runner;

pub use runner::{JobHandler, JobRunner};

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, Row, TransactionBehavior};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    FileTranscription,
    Minutes,
}

impl JobKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FileTranscription => "file_transcription",
            Self::Minutes => "minutes",
        }
    }

    fn parse(value: &str) -> Result<Self, JobError> {
        match value {
            "file_transcription" => Ok(Self::FileTranscription),
            "minutes" => Ok(Self::Minutes),
            _ => Err(JobError::InvalidJobKind(value.to_string())),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Superseded,
}

impl JobStatus {
    fn parse(value: &str) -> Result<Self, JobError> {
        match value {
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "superseded" => Ok(Self::Superseded),
            _ => Err(JobError::InvalidJobStatus(value.to_string())),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistentJob {
    pub id: String,
    pub meeting_id: String,
    pub kind: JobKind,
    pub status: JobStatus,
    pub attempts: u32,
    pub input_revision: Option<u64>,
    pub error_summary: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewPersistentJob {
    pub id: String,
    pub meeting_id: String,
    pub kind: JobKind,
    pub input_revision: Option<u64>,
    pub created_at: String,
}

impl NewPersistentJob {
    pub fn new(
        id: impl Into<String>,
        meeting_id: impl Into<String>,
        kind: JobKind,
        input_revision: Option<u64>,
        created_at: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            meeting_id: meeting_id.into(),
            kind,
            input_revision,
            created_at: created_at.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JobOutput {
    Transcription {
        transcript_revision: u64,
        text: String,
    },
    Minutes {
        transcript_revision: u64,
        content: String,
        provider_label: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnqueueDisposition {
    Enqueued,
    Coalesced,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JobRunDisposition {
    Idle,
    Completed,
    Retrying,
    Failed,
    Superseded,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetryPolicy {
    max_attempts: u32,
}

impl RetryPolicy {
    pub fn new(max_attempts: u32) -> Self {
        Self {
            max_attempts: max_attempts.max(1),
        }
    }

    pub fn max_attempts(self) -> u32 {
        self.max_attempts
    }
}

#[derive(Debug, thiserror::Error)]
pub enum JobError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("invalid job kind: {0}")]
    InvalidJobKind(String),
    #[error("invalid job status: {0}")]
    InvalidJobStatus(String),
    #[error("job {0} was not found")]
    NotFound(String),
    #[error("job {0} is not running")]
    NotRunning(String),
    #[error("job {0} requires an input revision")]
    MissingRevision(String),
    #[error("job output revision does not match its input revision")]
    RevisionMismatch,
    #[error("job output does not match its kind")]
    OutputKindMismatch,
}

pub struct SqliteJobStore {
    connection: Connection,
}

impl SqliteJobStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, JobError> {
        let connection = Connection::open(path)?;
        connection.pragma_update(None, "foreign_keys", true)?;
        Ok(Self { connection })
    }

    pub fn enqueue(&mut self, job: NewPersistentJob) -> Result<EnqueueDisposition, JobError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if job.kind == JobKind::FileTranscription {
            let active_exists: bool = transaction.query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM processing_jobs
                    WHERE meeting_id = ?1 AND job_type = 'file_transcription'
                      AND status IN ('queued', 'running')
                 )",
                params![job.meeting_id],
                |row| row.get(0),
            )?;
            if active_exists {
                return Ok(EnqueueDisposition::Coalesced);
            }
        }
        if job.kind == JobKind::Minutes {
            let revision = required_revision(&job.id, job.input_revision)?;
            let newest: Option<i64> = transaction
                .query_row(
                    "SELECT MAX(input_revision) FROM processing_jobs
                     WHERE meeting_id = ?1 AND job_type = 'minutes'
                       AND status IN ('queued', 'running')",
                    params![job.meeting_id],
                    |row| row.get(0),
                )
                .optional()?
                .flatten();
            if newest.is_some_and(|current| current >= revision as i64) {
                return Ok(EnqueueDisposition::Coalesced);
            }
            transaction.execute(
                "UPDATE processing_jobs
                 SET status = 'superseded', updated_at = ?3
                 WHERE meeting_id = ?1 AND job_type = 'minutes' AND status = 'queued'
                   AND COALESCE(input_revision, 0) < ?2",
                params![job.meeting_id, revision as i64, job.created_at],
            )?;
        }

        transaction.execute(
            "INSERT INTO processing_jobs (
                id, meeting_id, job_type, status, attempts, input_revision, created_at, updated_at
             ) VALUES (?1, ?2, ?3, 'queued', 0, ?4, ?5, ?5)",
            params![
                job.id,
                job.meeting_id,
                job.kind.as_str(),
                job.input_revision.map(|revision| revision as i64),
                job.created_at,
            ],
        )?;
        set_meeting_job_state(
            &transaction,
            &job.meeting_id,
            job.kind,
            "pending",
            &job.created_at,
        )?;
        transaction.commit()?;
        Ok(EnqueueDisposition::Enqueued)
    }

    pub fn claim_next(
        &mut self,
        kind: JobKind,
        updated_at: &str,
    ) -> Result<Option<PersistentJob>, JobError> {
        let transaction = self.connection.transaction()?;
        let claimed = transaction
            .query_row(
                "UPDATE processing_jobs
                 SET status = 'running', attempts = attempts + 1, updated_at = ?2,
                     error_summary = NULL
                 WHERE id = (
                    SELECT id FROM processing_jobs
                    WHERE job_type = ?1 AND status = 'queued'
                    ORDER BY created_at, id LIMIT 1
                 )
                 RETURNING id, meeting_id, job_type, status, attempts, input_revision, error_summary",
                params![kind.as_str(), updated_at],
                persistent_job_from_row,
            )
            .optional()?;
        if let Some(job) = claimed.as_ref() {
            set_meeting_job_state(
                &transaction,
                &job.meeting_id,
                kind,
                "processing",
                updated_at,
            )?;
        }
        transaction.commit()?;
        Ok(claimed)
    }

    pub fn recover_interrupted(&mut self, updated_at: &str) -> Result<usize, JobError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let interrupted = {
            let mut statement = transaction.prepare(
                "SELECT meeting_id, job_type FROM processing_jobs WHERE status = 'running'",
            )?;
            let rows = statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        let recovered = transaction.execute(
            "UPDATE processing_jobs
             SET status = 'queued',
                 error_summary = '应用上次退出时任务中断，已自动重新排队。',
                 updated_at = ?1
             WHERE status = 'running'",
            params![updated_at],
        )?;
        for (meeting_id, kind) in interrupted {
            set_meeting_job_state(
                &transaction,
                &meeting_id,
                JobKind::parse(&kind)?,
                "pending",
                updated_at,
            )?;
        }
        transaction.commit()?;
        Ok(recovered)
    }

    pub fn job(&self, job_id: &str) -> Result<Option<PersistentJob>, JobError> {
        self.connection
            .query_row(
                "SELECT id, meeting_id, job_type, status, attempts, input_revision, error_summary
                 FROM processing_jobs WHERE id = ?1",
                params![job_id],
                persistent_job_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn list_for_meeting(&self, meeting_id: &str) -> Result<Vec<PersistentJob>, JobError> {
        let mut statement = self.connection.prepare(
            "SELECT id, meeting_id, job_type, status, attempts, input_revision, error_summary
             FROM processing_jobs WHERE meeting_id = ?1 ORDER BY created_at DESC, id DESC",
        )?;
        let rows = statement.query_map(params![meeting_id], persistent_job_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn finish_failure(
        &mut self,
        job: &PersistentJob,
        max_attempts: u32,
        error: &str,
        updated_at: &str,
    ) -> Result<JobStatus, JobError> {
        self.require_running(job)?;
        let status = if job.attempts < max_attempts.max(1) {
            JobStatus::Queued
        } else {
            JobStatus::Failed
        };
        let status_value = match status {
            JobStatus::Queued => "queued",
            JobStatus::Failed => "failed",
            _ => unreachable!(),
        };
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "UPDATE processing_jobs
             SET status = ?2, error_summary = ?3, updated_at = ?4
             WHERE id = ?1 AND status = 'running'",
            params![
                job.id,
                status_value,
                sanitize_error_summary(error),
                updated_at
            ],
        )?;
        let meeting_state = if status == JobStatus::Failed {
            "failed"
        } else {
            "pending"
        };
        set_meeting_job_state(
            &transaction,
            &job.meeting_id,
            job.kind,
            meeting_state,
            updated_at,
        )?;
        transaction.commit()?;
        Ok(status)
    }

    pub fn retry_latest_failed(
        &mut self,
        meeting_id: &str,
        kind: JobKind,
        input_revision: Option<u64>,
        updated_at: &str,
    ) -> Result<PersistentJob, JobError> {
        let transaction = self.connection.transaction()?;
        let failed_id: Option<String> = transaction
            .query_row(
                "SELECT id FROM processing_jobs
                 WHERE meeting_id = ?1 AND job_type = ?2 AND status = 'failed'
                   AND (?3 IS NULL OR input_revision = ?3)
                 ORDER BY updated_at DESC, id DESC LIMIT 1",
                params![
                    meeting_id,
                    kind.as_str(),
                    input_revision.map(|value| value as i64)
                ],
                |row| row.get(0),
            )
            .optional()?;
        let job_id = failed_id.ok_or_else(|| JobError::NotFound(meeting_id.to_string()))?;
        transaction.execute(
            "UPDATE processing_jobs
             SET status = 'queued', attempts = 0, error_summary = NULL, updated_at = ?2
             WHERE id = ?1",
            params![job_id, updated_at],
        )?;
        set_meeting_job_state(&transaction, meeting_id, kind, "pending", updated_at)?;
        let job = transaction.query_row(
            "SELECT id, meeting_id, job_type, status, attempts, input_revision, error_summary
             FROM processing_jobs WHERE id = ?1",
            params![job_id],
            persistent_job_from_row,
        )?;
        transaction.commit()?;
        Ok(job)
    }

    pub(crate) fn finish_success(
        &mut self,
        job: &PersistentJob,
        output: JobOutput,
        updated_at: &str,
    ) -> Result<JobRunDisposition, JobError> {
        self.require_running(job)?;
        let transaction = self.connection.transaction()?;
        let disposition = match output {
            JobOutput::Transcription {
                transcript_revision,
                text,
            } if job.kind == JobKind::FileTranscription => {
                verify_revision(job, transcript_revision)?;
                let current = latest_transcript_revision(&transaction, &job.meeting_id)?;
                if current >= transcript_revision as i64 {
                    mark_superseded(&transaction, &job.id, updated_at)?;
                    JobRunDisposition::Superseded
                } else {
                    transaction.execute(
                        "INSERT INTO transcript_segments (
                            id, meeting_id, revision, start_ms, end_ms, text, status, created_at
                         ) VALUES (?1, ?2, ?3, 0, 0, ?4, 'final', ?5)",
                        params![
                            format!("{}-result", job.id),
                            job.meeting_id,
                            transcript_revision as i64,
                            text,
                            updated_at,
                        ],
                    )?;
                    mark_succeeded(&transaction, &job.id, updated_at)?;
                    set_meeting_job_state(
                        &transaction,
                        &job.meeting_id,
                        job.kind,
                        "ready",
                        updated_at,
                    )?;
                    JobRunDisposition::Completed
                }
            }
            JobOutput::Minutes {
                transcript_revision,
                content,
                provider_label,
            } if job.kind == JobKind::Minutes => {
                verify_revision(job, transcript_revision)?;
                let current_transcript = latest_transcript_revision(&transaction, &job.meeting_id)?;
                let current_minutes: i64 = transaction.query_row(
                    "SELECT COALESCE(MAX(revision), 0) FROM meeting_minutes WHERE meeting_id = ?1",
                    params![job.meeting_id],
                    |row| row.get(0),
                )?;
                let newer_job: Option<i64> = transaction
                    .query_row(
                        "SELECT MAX(input_revision) FROM processing_jobs
                         WHERE meeting_id = ?1 AND job_type = 'minutes' AND id <> ?2
                           AND status IN ('queued', 'running')",
                        params![job.meeting_id, job.id],
                        |row| row.get(0),
                    )
                    .optional()?
                    .flatten();
                let stale = current_transcript > transcript_revision as i64
                    || current_minutes >= transcript_revision as i64
                    || newer_job.is_some_and(|revision| revision > transcript_revision as i64);
                if stale {
                    mark_superseded(&transaction, &job.id, updated_at)?;
                    JobRunDisposition::Superseded
                } else if current_transcript < transcript_revision as i64 {
                    return Err(JobError::RevisionMismatch);
                } else {
                    transaction.execute(
                        "INSERT INTO meeting_minutes (
                            id, meeting_id, revision, content, provider_label, status, created_at
                         ) VALUES (?1, ?2, ?3, ?4, ?5, 'ready', ?6)",
                        params![
                            format!("{}-result", job.id),
                            job.meeting_id,
                            transcript_revision as i64,
                            content,
                            provider_label,
                            updated_at,
                        ],
                    )?;
                    mark_succeeded(&transaction, &job.id, updated_at)?;
                    set_meeting_job_state(
                        &transaction,
                        &job.meeting_id,
                        job.kind,
                        "ready",
                        updated_at,
                    )?;
                    JobRunDisposition::Completed
                }
            }
            _ => return Err(JobError::OutputKindMismatch),
        };
        transaction.commit()?;
        Ok(disposition)
    }

    fn require_running(&self, job: &PersistentJob) -> Result<(), JobError> {
        let current = self
            .job(&job.id)?
            .ok_or_else(|| JobError::NotFound(job.id.clone()))?;
        if current.status == JobStatus::Running {
            Ok(())
        } else {
            Err(JobError::NotRunning(job.id.clone()))
        }
    }
}

fn persistent_job_from_row(row: &Row<'_>) -> rusqlite::Result<PersistentJob> {
    let kind = row.get::<_, String>(2)?;
    let status = row.get::<_, String>(3)?;
    Ok(PersistentJob {
        id: row.get(0)?,
        meeting_id: row.get(1)?,
        kind: JobKind::parse(&kind).map_err(to_sql_conversion_error)?,
        status: JobStatus::parse(&status).map_err(to_sql_conversion_error)?,
        attempts: row.get::<_, i64>(4)? as u32,
        input_revision: row.get::<_, Option<i64>>(5)?.map(|value| value as u64),
        error_summary: row.get(6)?,
    })
}

fn to_sql_conversion_error(error: JobError) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}

fn required_revision(job_id: &str, revision: Option<u64>) -> Result<u64, JobError> {
    revision.ok_or_else(|| JobError::MissingRevision(job_id.to_string()))
}

fn verify_revision(job: &PersistentJob, output_revision: u64) -> Result<(), JobError> {
    if required_revision(&job.id, job.input_revision)? == output_revision {
        Ok(())
    } else {
        Err(JobError::RevisionMismatch)
    }
}

fn latest_transcript_revision(
    connection: &Connection,
    meeting_id: &str,
) -> Result<i64, rusqlite::Error> {
    connection.query_row(
        "SELECT COALESCE(MAX(revision), 0) FROM transcript_segments WHERE meeting_id = ?1",
        params![meeting_id],
        |row| row.get(0),
    )
}

fn mark_succeeded(
    connection: &Connection,
    job_id: &str,
    updated_at: &str,
) -> Result<(), rusqlite::Error> {
    connection.execute(
        "UPDATE processing_jobs
         SET status = 'succeeded', error_summary = NULL, updated_at = ?2 WHERE id = ?1",
        params![job_id, updated_at],
    )?;
    Ok(())
}

fn mark_superseded(
    connection: &Connection,
    job_id: &str,
    updated_at: &str,
) -> Result<(), rusqlite::Error> {
    connection.execute(
        "UPDATE processing_jobs
         SET status = 'superseded', error_summary = NULL, updated_at = ?2 WHERE id = ?1",
        params![job_id, updated_at],
    )?;
    Ok(())
}

fn set_meeting_job_state(
    connection: &Connection,
    meeting_id: &str,
    kind: JobKind,
    state: &str,
    updated_at: &str,
) -> Result<(), rusqlite::Error> {
    let column = match kind {
        JobKind::FileTranscription => "transcription_status",
        JobKind::Minutes => "minutes_status",
    };
    connection.execute(
        &format!("UPDATE meeting_records SET {column} = ?2, updated_at = ?3 WHERE id = ?1"),
        params![meeting_id, state, updated_at],
    )?;
    Ok(())
}

fn sanitize_error_summary(error: &str) -> String {
    let mut redact_next = false;
    let mut sanitized = error.to_string();
    for marker in ["api_key=", "api-key=", "token=", "access_token="] {
        redact_assignments(&mut sanitized, marker);
    }
    sanitized
        .split_whitespace()
        .map(|word| {
            let lower = word.to_ascii_lowercase();
            if redact_next {
                redact_next = false;
                return "[REDACTED]".to_string();
            }
            if lower == "bearer" {
                redact_next = true;
                return word.to_string();
            }
            if lower.starts_with("sk-")
                || lower.starts_with("api_key=")
                || lower.starts_with("api-key=")
                || lower.starts_with("token=")
            {
                "[REDACTED]".to_string()
            } else {
                word.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(240)
        .collect()
}

fn redact_assignments(value: &mut String, marker: &str) {
    let mut search_from = 0;
    loop {
        let lower = value.to_ascii_lowercase();
        let Some(relative_start) = lower[search_from..].find(marker) else {
            return;
        };
        let secret_start = search_from + relative_start + marker.len();
        let secret_end = value[secret_start..]
            .find(|character: char| character.is_whitespace() || matches!(character, '&' | '#'))
            .map(|offset| secret_start + offset)
            .unwrap_or(value.len());
        value.replace_range(secret_start..secret_end, "[REDACTED]");
        search_from = secret_start + "[REDACTED]".len();
    }
}
