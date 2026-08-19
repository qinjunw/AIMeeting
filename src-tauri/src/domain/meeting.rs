use std::fmt;

use serde::{Deserialize, Serialize};

use super::{TranscriptionUpdate, UpdateDisposition};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordingStatus {
    Preparing,
    Recording,
    Paused,
    Stopping,
    Processing,
    Ready,
    Interrupted,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptionStatus {
    Pending,
    Streaming,
    Processing,
    Ready,
    Failed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MinutesStatus {
    Pending,
    Processing,
    Ready,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingRun {
    pub generation: u64,
    pub closed: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingRecord {
    pub id: String,
    pub recording_status: RecordingStatus,
    pub transcription_status: TranscriptionStatus,
    pub minutes_status: MinutesStatus,
    pub run_generation: u64,
    pub transcript_revision: u64,
    pub transcription_error: Option<String>,
    pub recording_runs: Vec<RecordingRun>,
}

impl MeetingRecord {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            recording_status: RecordingStatus::Preparing,
            transcription_status: TranscriptionStatus::Pending,
            minutes_status: MinutesStatus::Pending,
            run_generation: 0,
            transcript_revision: 0,
            transcription_error: None,
            recording_runs: Vec::new(),
        }
    }

    pub fn start_recording(&mut self) -> Result<(), DomainError> {
        self.require_status(RecordingStatus::Preparing, RecordingStatus::Recording)?;
        self.open_run();
        self.recording_status = RecordingStatus::Recording;
        Ok(())
    }

    pub fn pause(&mut self) -> Result<(), DomainError> {
        self.require_status(RecordingStatus::Recording, RecordingStatus::Paused)?;
        self.close_current_run();
        self.recording_status = RecordingStatus::Paused;
        Ok(())
    }

    pub fn resume(&mut self) -> Result<(), DomainError> {
        self.require_status(RecordingStatus::Paused, RecordingStatus::Recording)?;
        self.open_run();
        self.recording_status = RecordingStatus::Recording;
        Ok(())
    }

    pub fn request_stop(&mut self) -> Result<bool, DomainError> {
        match self.recording_status {
            RecordingStatus::Recording | RecordingStatus::Paused => {
                self.close_current_run();
                self.recording_status = RecordingStatus::Stopping;
                Ok(true)
            }
            RecordingStatus::Stopping | RecordingStatus::Processing | RecordingStatus::Ready => {
                Ok(false)
            }
            from => Err(DomainError::InvalidRecordingTransition {
                from,
                to: RecordingStatus::Stopping,
            }),
        }
    }

    pub fn begin_processing(&mut self) -> Result<(), DomainError> {
        self.require_status(RecordingStatus::Stopping, RecordingStatus::Processing)?;
        self.recording_status = RecordingStatus::Processing;
        Ok(())
    }

    pub fn mark_ready(&mut self) -> Result<(), DomainError> {
        self.require_status(RecordingStatus::Processing, RecordingStatus::Ready)?;
        self.recording_status = RecordingStatus::Ready;
        Ok(())
    }

    pub fn mark_interrupted(&mut self) -> Result<(), DomainError> {
        match self.recording_status {
            RecordingStatus::Preparing
            | RecordingStatus::Recording
            | RecordingStatus::Paused
            | RecordingStatus::Stopping
            | RecordingStatus::Processing => {
                self.close_current_run();
                self.recording_status = RecordingStatus::Interrupted;
                Ok(())
            }
            from => Err(DomainError::InvalidRecordingTransition {
                from,
                to: RecordingStatus::Interrupted,
            }),
        }
    }

    pub fn recover_interrupted(&mut self) -> Result<(), DomainError> {
        self.require_status(RecordingStatus::Interrupted, RecordingStatus::Paused)?;
        self.recording_status = RecordingStatus::Paused;
        Ok(())
    }

    pub fn mark_transcription_streaming(&mut self) {
        self.transcription_status = TranscriptionStatus::Streaming;
        self.transcription_error = None;
    }

    pub fn mark_transcription_failed(&mut self, message: impl Into<String>) {
        self.transcription_status = TranscriptionStatus::Failed;
        self.transcription_error = Some(message.into());
    }

    pub fn apply_transcription_update(
        &mut self,
        update: &TranscriptionUpdate,
    ) -> UpdateDisposition {
        if update.run_generation != self.run_generation {
            return UpdateDisposition::IgnoredStaleGeneration;
        }
        if update.revision < self.transcript_revision {
            return UpdateDisposition::IgnoredStaleRevision;
        }

        self.transcription_status = update.status;
        self.transcript_revision = update.revision;
        if update.status != TranscriptionStatus::Failed {
            self.transcription_error = None;
        }
        UpdateDisposition::Applied
    }

    fn require_status(
        &self,
        expected: RecordingStatus,
        target: RecordingStatus,
    ) -> Result<(), DomainError> {
        if self.recording_status == expected {
            Ok(())
        } else {
            Err(DomainError::InvalidRecordingTransition {
                from: self.recording_status,
                to: target,
            })
        }
    }

    fn open_run(&mut self) {
        self.run_generation += 1;
        self.recording_runs.push(RecordingRun {
            generation: self.run_generation,
            closed: false,
        });
    }

    fn close_current_run(&mut self) {
        if let Some(run) = self.recording_runs.last_mut() {
            run.closed = true;
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DomainError {
    InvalidRecordingTransition {
        from: RecordingStatus,
        to: RecordingStatus,
    },
}

impl fmt::Display for DomainError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRecordingTransition { from, to } => {
                write!(
                    formatter,
                    "invalid recording transition from {from:?} to {to:?}"
                )
            }
        }
    }
}

impl std::error::Error for DomainError {}
