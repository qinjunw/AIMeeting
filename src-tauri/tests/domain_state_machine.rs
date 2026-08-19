#[path = "../src/domain/mod.rs"]
mod domain;
#[path = "../src/meeting/mod.rs"]
mod meeting;

use std::collections::HashMap;

use domain::{
    DomainError, MeetingRecord, MinutesJob, MinutesStatus, ProviderCapability, RecordingRun,
    RecordingStatus, TranscriptionJob, TranscriptionStatus, TranscriptionUpdate, UpdateDisposition,
};
use meeting::{
    AudioRuntime, MeetingRepository, MeetingService, ServiceError, TranscriptionGateway,
};

#[test]
fn recording_follows_the_complete_happy_path() {
    let mut meeting = MeetingRecord::new("meeting-1");

    assert_eq!(meeting.recording_status, RecordingStatus::Preparing);
    assert_eq!(meeting.start_recording(), Ok(()));
    assert_eq!(meeting.recording_status, RecordingStatus::Recording);
    assert_eq!(meeting.run_generation, 1);

    assert_eq!(meeting.pause(), Ok(()));
    assert_eq!(meeting.recording_status, RecordingStatus::Paused);

    assert_eq!(meeting.resume(), Ok(()));
    assert_eq!(meeting.recording_status, RecordingStatus::Recording);
    assert_eq!(meeting.run_generation, 2);
    assert_eq!(meeting.recording_runs.len(), 2);
    let runs: &[RecordingRun] = &meeting.recording_runs;
    assert_eq!(runs[0].generation, 1);
    assert!(runs[0].closed);
    assert_eq!(runs[1].generation, 2);

    assert_eq!(meeting.request_stop(), Ok(true));
    assert_eq!(meeting.recording_status, RecordingStatus::Stopping);
    assert_eq!(meeting.begin_processing(), Ok(()));
    assert_eq!(meeting.recording_status, RecordingStatus::Processing);
    assert_eq!(meeting.mark_ready(), Ok(()));
    assert_eq!(meeting.recording_status, RecordingStatus::Ready);
}

#[test]
fn illegal_transition_reports_the_source_and_target() {
    let mut meeting = MeetingRecord::new("meeting-1");

    assert_eq!(
        meeting.pause(),
        Err(DomainError::InvalidRecordingTransition {
            from: RecordingStatus::Preparing,
            to: RecordingStatus::Paused,
        })
    );
    assert_eq!(meeting.recording_status, RecordingStatus::Preparing);
}

#[test]
fn stop_is_idempotent_after_the_first_request() {
    let mut meeting = MeetingRecord::new("meeting-1");
    meeting.start_recording().unwrap();

    assert_eq!(meeting.request_stop(), Ok(true));
    assert_eq!(meeting.request_stop(), Ok(false));
    assert_eq!(meeting.recording_status, RecordingStatus::Stopping);
}

#[test]
fn interrupted_recording_recovers_as_paused_without_reusing_a_generation() {
    let mut meeting = MeetingRecord::new("meeting-1");
    meeting.start_recording().unwrap();

    meeting.mark_interrupted().unwrap();
    assert_eq!(meeting.recording_status, RecordingStatus::Interrupted);
    assert_eq!(meeting.run_generation, 1);

    meeting.recover_interrupted().unwrap();
    assert_eq!(meeting.recording_status, RecordingStatus::Paused);

    meeting.resume().unwrap();
    assert_eq!(meeting.recording_status, RecordingStatus::Recording);
    assert_eq!(meeting.run_generation, 2);
}

#[test]
fn asr_failure_does_not_change_recording_or_minutes_state() {
    let mut meeting = MeetingRecord::new("meeting-1");
    meeting.start_recording().unwrap();

    meeting.mark_transcription_failed("provider unavailable");

    assert_eq!(meeting.recording_status, RecordingStatus::Recording);
    assert_eq!(meeting.transcription_status, TranscriptionStatus::Failed);
    assert_eq!(meeting.minutes_status, MinutesStatus::Pending);
    assert_eq!(
        meeting.transcription_error.as_deref(),
        Some("provider unavailable")
    );
}

#[test]
fn statuses_serialize_as_snake_case_json_values() {
    assert_eq!(
        serde_json::to_string(&RecordingStatus::Interrupted).unwrap(),
        "\"interrupted\""
    );
    assert_eq!(
        serde_json::to_string(&TranscriptionStatus::Streaming).unwrap(),
        "\"streaming\""
    );
    assert_eq!(
        serde_json::to_string(&MinutesStatus::Processing).unwrap(),
        "\"processing\""
    );
    assert_eq!(
        serde_json::to_string(&ProviderCapability::LiveTranscription).unwrap(),
        "\"live_transcription\""
    );
}

#[test]
fn jobs_identify_the_run_and_transcript_revision_they_own() {
    let transcription = TranscriptionJob {
        meeting_id: "meeting-1".to_string(),
        run_generation: 3,
        revision: 7,
    };
    let minutes = MinutesJob {
        meeting_id: "meeting-1".to_string(),
        transcript_revision: 7,
        status: MinutesStatus::Pending,
    };

    let transcription_json = serde_json::to_value(transcription).unwrap();
    let minutes_json = serde_json::to_value(minutes).unwrap();
    assert_eq!(transcription_json["runGeneration"], 3);
    assert_eq!(transcription_json["revision"], 7);
    assert_eq!(minutes_json["transcriptRevision"], 7);
}

#[test]
fn service_degrades_transcription_when_gateway_start_fails() {
    let mut repository = FakeRepository::with(MeetingRecord::new("meeting-1"));
    let mut audio = FakeAudioRuntime::default();
    let mut gateway = FakeTranscriptionGateway {
        fail_start: true,
        ..Default::default()
    };

    let meeting = {
        let mut service = MeetingService::new(&mut repository, &mut audio, &mut gateway);
        service.start("meeting-1").unwrap()
    };

    assert_eq!(meeting.recording_status, RecordingStatus::Recording);
    assert_eq!(meeting.transcription_status, TranscriptionStatus::Failed);
    assert_eq!(audio.started_generations, vec![1]);
    assert_eq!(gateway.started_generations, vec![1]);
}

#[test]
fn service_stop_calls_runtime_once_when_requested_repeatedly() {
    let mut meeting = MeetingRecord::new("meeting-1");
    meeting.start_recording().unwrap();
    let mut repository = FakeRepository::with(meeting);
    let mut audio = FakeAudioRuntime::default();
    let mut gateway = FakeTranscriptionGateway::default();

    {
        let mut service = MeetingService::new(&mut repository, &mut audio, &mut gateway);
        service.stop("meeting-1").unwrap();
        service.stop("meeting-1").unwrap();
    }

    assert_eq!(audio.stop_calls, 1);
    assert_eq!(gateway.stop_calls, 1);
    assert_eq!(
        repository.records["meeting-1"].recording_status,
        RecordingStatus::Stopping
    );
}

#[test]
fn service_pauses_and_resumes_with_a_new_run_generation() {
    let mut repository = FakeRepository::with(MeetingRecord::new("meeting-1"));
    let mut audio = FakeAudioRuntime::default();
    let mut gateway = FakeTranscriptionGateway::default();

    let resumed = {
        let mut service = MeetingService::new(&mut repository, &mut audio, &mut gateway);
        service.start("meeting-1").unwrap();
        service.pause("meeting-1").unwrap();
        service.resume("meeting-1").unwrap()
    };

    assert_eq!(resumed.recording_status, RecordingStatus::Recording);
    assert_eq!(resumed.run_generation, 2);
    assert_eq!(audio.pause_calls, 1);
    assert_eq!(audio.started_generations, vec![1, 2]);
    assert_eq!(gateway.started_generations, vec![1, 2]);
}

#[test]
fn service_reports_a_missing_meeting_without_touching_dependencies() {
    let mut repository = FakeRepository::default();
    let mut audio = FakeAudioRuntime::default();
    let mut gateway = FakeTranscriptionGateway::default();

    let result = {
        let mut service = MeetingService::new(&mut repository, &mut audio, &mut gateway);
        service.start("missing")
    };

    assert_eq!(result, Err(ServiceError::NotFound("missing".to_string())));
    assert!(audio.started_generations.is_empty());
    assert!(gateway.started_generations.is_empty());
}

#[test]
fn service_ignores_transcription_updates_from_an_old_run_generation() {
    let mut meeting = MeetingRecord::new("meeting-1");
    meeting.start_recording().unwrap();
    meeting.pause().unwrap();
    meeting.resume().unwrap();
    assert_eq!(meeting.run_generation, 2);

    let mut repository = FakeRepository::with(meeting);
    let mut audio = FakeAudioRuntime::default();
    let mut gateway = FakeTranscriptionGateway::default();
    let stale = TranscriptionUpdate::new("meeting-1", 1, 1, TranscriptionStatus::Ready);
    let current = TranscriptionUpdate::new("meeting-1", 2, 1, TranscriptionStatus::Ready);

    let (stale_result, current_result) = {
        let mut service = MeetingService::new(&mut repository, &mut audio, &mut gateway);
        (
            service.apply_transcription_update(&stale).unwrap(),
            service.apply_transcription_update(&current).unwrap(),
        )
    };

    assert_eq!(stale_result, UpdateDisposition::IgnoredStaleGeneration);
    assert_eq!(current_result, UpdateDisposition::Applied);
    assert_eq!(
        repository.records["meeting-1"].transcription_status,
        TranscriptionStatus::Ready
    );
    assert_eq!(repository.records["meeting-1"].transcript_revision, 1);
}

#[test]
fn service_ignores_an_older_revision_from_the_current_run() {
    let mut meeting = MeetingRecord::new("meeting-1");
    meeting.start_recording().unwrap();
    meeting.transcript_revision = 4;
    meeting.transcription_status = TranscriptionStatus::Ready;

    let mut repository = FakeRepository::with(meeting);
    let mut audio = FakeAudioRuntime::default();
    let mut gateway = FakeTranscriptionGateway::default();
    let stale = TranscriptionUpdate::new("meeting-1", 1, 3, TranscriptionStatus::Processing);

    let disposition = {
        let mut service = MeetingService::new(&mut repository, &mut audio, &mut gateway);
        service.apply_transcription_update(&stale).unwrap()
    };

    assert_eq!(disposition, UpdateDisposition::IgnoredStaleRevision);
    assert_eq!(repository.records["meeting-1"].transcript_revision, 4);
    assert_eq!(
        repository.records["meeting-1"].transcription_status,
        TranscriptionStatus::Ready
    );
}

#[derive(Default)]
struct FakeRepository {
    records: HashMap<String, MeetingRecord>,
}

impl FakeRepository {
    fn with(meeting: MeetingRecord) -> Self {
        Self {
            records: HashMap::from([(meeting.id.clone(), meeting)]),
        }
    }
}

impl MeetingRepository for FakeRepository {
    fn load(&self, meeting_id: &str) -> Result<Option<MeetingRecord>, String> {
        Ok(self.records.get(meeting_id).cloned())
    }

    fn save(&mut self, meeting: &MeetingRecord) -> Result<(), String> {
        self.records.insert(meeting.id.clone(), meeting.clone());
        Ok(())
    }
}

#[derive(Default)]
struct FakeAudioRuntime {
    started_generations: Vec<u64>,
    pause_calls: usize,
    stop_calls: usize,
}

impl AudioRuntime for FakeAudioRuntime {
    fn start_run(&mut self, _meeting_id: &str, generation: u64) -> Result<(), String> {
        self.started_generations.push(generation);
        Ok(())
    }

    fn pause(&mut self, _meeting_id: &str, _generation: u64) -> Result<(), String> {
        self.pause_calls += 1;
        Ok(())
    }

    fn stop(&mut self, _meeting_id: &str, _generation: u64) -> Result<(), String> {
        self.stop_calls += 1;
        Ok(())
    }
}

#[derive(Default)]
struct FakeTranscriptionGateway {
    started_generations: Vec<u64>,
    stop_calls: usize,
    fail_start: bool,
}

impl TranscriptionGateway for FakeTranscriptionGateway {
    fn start_run(&mut self, _meeting_id: &str, generation: u64) -> Result<(), String> {
        self.started_generations.push(generation);
        if self.fail_start {
            Err("provider unavailable".to_string())
        } else {
            Ok(())
        }
    }

    fn stop_run(&mut self, _meeting_id: &str, _generation: u64) -> Result<(), String> {
        self.stop_calls += 1;
        Ok(())
    }
}
