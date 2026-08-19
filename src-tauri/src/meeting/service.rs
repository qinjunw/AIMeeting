use std::fmt;

use crate::domain::{DomainError, MeetingRecord, TranscriptionUpdate, UpdateDisposition};

pub trait MeetingRepository {
    fn load(&self, meeting_id: &str) -> Result<Option<MeetingRecord>, String>;
    fn save(&mut self, meeting: &MeetingRecord) -> Result<(), String>;
}

pub trait AudioRuntime {
    fn start_run(&mut self, meeting_id: &str, generation: u64) -> Result<(), String>;
    fn pause(&mut self, meeting_id: &str, generation: u64) -> Result<(), String>;
    fn stop(&mut self, meeting_id: &str, generation: u64) -> Result<(), String>;
}

pub trait TranscriptionGateway {
    fn start_run(&mut self, meeting_id: &str, generation: u64) -> Result<(), String>;
    fn stop_run(&mut self, meeting_id: &str, generation: u64) -> Result<(), String>;
}

pub struct MeetingService<'a> {
    repository: &'a mut dyn MeetingRepository,
    audio: &'a mut dyn AudioRuntime,
    transcription: &'a mut dyn TranscriptionGateway,
}

impl<'a> MeetingService<'a> {
    pub fn new(
        repository: &'a mut dyn MeetingRepository,
        audio: &'a mut dyn AudioRuntime,
        transcription: &'a mut dyn TranscriptionGateway,
    ) -> Self {
        Self {
            repository,
            audio,
            transcription,
        }
    }

    pub fn start(&mut self, meeting_id: &str) -> Result<MeetingRecord, ServiceError> {
        let mut meeting = self.load(meeting_id)?;
        meeting.start_recording()?;
        self.audio
            .start_run(meeting_id, meeting.run_generation)
            .map_err(ServiceError::Audio)?;
        self.start_transcription_best_effort(&mut meeting);
        self.save(&meeting)?;
        Ok(meeting)
    }

    pub fn pause(&mut self, meeting_id: &str) -> Result<MeetingRecord, ServiceError> {
        let mut meeting = self.load(meeting_id)?;
        meeting.pause()?;
        self.audio
            .pause(meeting_id, meeting.run_generation)
            .map_err(ServiceError::Audio)?;
        self.save(&meeting)?;
        Ok(meeting)
    }

    pub fn resume(&mut self, meeting_id: &str) -> Result<MeetingRecord, ServiceError> {
        let mut meeting = self.load(meeting_id)?;
        meeting.resume()?;
        self.audio
            .start_run(meeting_id, meeting.run_generation)
            .map_err(ServiceError::Audio)?;
        self.start_transcription_best_effort(&mut meeting);
        self.save(&meeting)?;
        Ok(meeting)
    }

    pub fn stop(&mut self, meeting_id: &str) -> Result<MeetingRecord, ServiceError> {
        let mut meeting = self.load(meeting_id)?;
        if !meeting.request_stop()? {
            return Ok(meeting);
        }

        self.audio
            .stop(meeting_id, meeting.run_generation)
            .map_err(ServiceError::Audio)?;
        if let Err(error) = self
            .transcription
            .stop_run(meeting_id, meeting.run_generation)
        {
            meeting.mark_transcription_failed(error);
        }
        self.save(&meeting)?;
        Ok(meeting)
    }

    pub fn apply_transcription_update(
        &mut self,
        update: &TranscriptionUpdate,
    ) -> Result<UpdateDisposition, ServiceError> {
        let mut meeting = self.load(&update.meeting_id)?;
        let disposition = meeting.apply_transcription_update(update);
        if disposition == UpdateDisposition::Applied {
            self.save(&meeting)?;
        }
        Ok(disposition)
    }

    fn start_transcription_best_effort(&mut self, meeting: &mut MeetingRecord) {
        match self
            .transcription
            .start_run(&meeting.id, meeting.run_generation)
        {
            Ok(()) => meeting.mark_transcription_streaming(),
            Err(error) => meeting.mark_transcription_failed(error),
        }
    }

    fn load(&self, meeting_id: &str) -> Result<MeetingRecord, ServiceError> {
        self.repository
            .load(meeting_id)
            .map_err(ServiceError::Repository)?
            .ok_or_else(|| ServiceError::NotFound(meeting_id.to_string()))
    }

    fn save(&mut self, meeting: &MeetingRecord) -> Result<(), ServiceError> {
        self.repository
            .save(meeting)
            .map_err(ServiceError::Repository)
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum ServiceError {
    NotFound(String),
    Domain(DomainError),
    Repository(String),
    Audio(String),
}

impl From<DomainError> for ServiceError {
    fn from(error: DomainError) -> Self {
        Self::Domain(error)
    }
}

impl fmt::Display for ServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(id) => write!(formatter, "meeting not found: {id}"),
            Self::Domain(error) => error.fmt(formatter),
            Self::Repository(error) => write!(formatter, "meeting repository error: {error}"),
            Self::Audio(error) => write!(formatter, "audio runtime error: {error}"),
        }
    }
}

impl std::error::Error for ServiceError {}
