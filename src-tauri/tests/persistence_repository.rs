use aimeeting_lib::persistence::{
    DataPaths, MeetingMinutesRow, MeetingRecordRow, MeetingRepository, NewMeetingRecord,
    NewProcessingJob, PersistenceError, ProcessingJobRow,
};
use tempfile::TempDir;

fn test_repository() -> (TempDir, MeetingRepository) {
    let temp = TempDir::new().expect("temporary directory");
    let paths = DataPaths::new(temp.path()).expect("data paths");
    let repository = MeetingRepository::open(paths.database_path()).expect("repository");
    (temp, repository)
}

fn meeting_record(id: &str, title: &str) -> NewMeetingRecord {
    NewMeetingRecord {
        id: id.to_string(),
        title: title.to_string(),
        status: "preparing".to_string(),
        transcription_status: "idle".to_string(),
        minutes_status: "idle".to_string(),
        created_at: "2026-08-19T00:00:00Z".to_string(),
    }
}

#[test]
fn migrations_create_the_complete_v1_schema() {
    let (_temp, repository) = test_repository();
    let tables = repository.table_names().expect("table names");

    for expected in [
        "meeting_records",
        "recording_runs",
        "recording_assets",
        "transcript_segments",
        "meeting_minutes",
        "processing_jobs",
        "provider_profiles",
        "app_settings",
    ] {
        assert!(tables.contains(&expected.to_string()), "missing {expected}");
    }
}

#[test]
fn soft_deleted_meetings_are_hidden_and_can_be_restored() {
    let (_temp, repository) = test_repository();
    repository
        .create_meeting(&meeting_record("meeting-1", "设计评审"))
        .expect("create meeting");

    let active: Vec<MeetingRecordRow> = repository.list_meetings(false).unwrap();
    assert_eq!(active.len(), 1);
    repository
        .soft_delete_meeting("meeting-1", "2026-08-19T12:00:00Z")
        .expect("soft delete");
    assert!(repository.list_meetings(false).unwrap().is_empty());
    assert_eq!(repository.list_meetings(true).unwrap().len(), 1);

    repository.restore_meeting("meeting-1").expect("restore");
    assert_eq!(repository.list_meetings(false).unwrap().len(), 1);
}

#[test]
fn meeting_processing_states_are_updated_together() {
    let (_temp, repository) = test_repository();
    repository
        .create_meeting(&meeting_record("meeting-1", "架构评审"))
        .unwrap();

    repository
        .update_meeting_states(
            "meeting-1",
            "processing",
            "ready",
            "processing",
            "2026-08-19T13:00:00Z",
        )
        .unwrap();

    let meeting = repository.list_meetings(false).unwrap().remove(0);
    assert_eq!(meeting.status, "processing");
    assert_eq!(meeting.transcription_status, "ready");
    assert_eq!(meeting.minutes_status, "processing");
    assert_eq!(meeting.updated_at, "2026-08-19T13:00:00Z");
}

#[test]
fn recording_lifecycle_metadata_is_persisted_with_the_local_asset() {
    let (_directory, repository) = test_repository();
    repository
        .create_meeting(&meeting_record("meeting-recording", "录音测试"))
        .unwrap();

    repository
        .create_recording_run(
            "meeting-recording-run-1",
            "meeting-recording",
            1,
            1,
            r#"{"microphone":true,"system":true}"#,
            "2026-08-19T00:00:00Z",
        )
        .unwrap();
    repository
        .finish_recording_run("meeting-recording-run-1", "2026-08-19T00:01:00Z")
        .unwrap();
    repository
        .upsert_recording_asset(
            "meeting-recording",
            "recording.opus",
            "ready",
            60_000,
            42_000,
            "2026-08-19T00:00:00Z",
        )
        .unwrap();
    repository
        .mark_meeting_stopped(
            "meeting-recording",
            "ready",
            "pending",
            "pending",
            "2026-08-19T00:01:00Z",
        )
        .unwrap();

    let meeting = repository
        .get_meeting("meeting-recording")
        .unwrap()
        .expect("meeting");
    assert_eq!(meeting.status, "ready");
    assert_eq!(meeting.stopped_at.as_deref(), Some("2026-08-19T00:01:00Z"));
    let asset = repository
        .latest_recording_asset("meeting-recording")
        .unwrap()
        .expect("recording asset");
    assert_eq!(asset.duration_ms, 60_000);
    assert_eq!(asset.byte_size, 42_000);
}

#[test]
fn transcript_revisions_are_append_only() {
    let (_temp, repository) = test_repository();
    repository
        .create_meeting(&meeting_record("meeting-1", "项目周会"))
        .expect("create meeting");

    assert_eq!(repository.next_transcript_revision("meeting-1").unwrap(), 1);
    repository
        .append_transcript_segment("segment-1", "meeting-1", 1, 0, 1200, "第一段")
        .expect("append segment");
    repository
        .append_transcript_segment("segment-2", "meeting-1", 1, 1200, 2400, "第二段")
        .expect("append segment");
    assert_eq!(repository.next_transcript_revision("meeting-1").unwrap(), 2);
    assert_eq!(
        repository.transcript_for_revision("meeting-1", 1).unwrap(),
        "第一段\n第二段"
    );
    assert_eq!(
        repository.full_transcript("meeting-1").unwrap(),
        "第一段\n第二段"
    );
}

#[test]
fn startup_recovery_marks_incomplete_meetings_interrupted() {
    let (_temp, repository) = test_repository();
    repository
        .create_meeting(&meeting_record("meeting-1", "未完成会议"))
        .unwrap();
    repository
        .update_meeting_states(
            "meeting-1",
            "recording",
            "streaming",
            "pending",
            "2026-08-19T12:00:00Z",
        )
        .unwrap();

    let recovered = repository
        .recover_incomplete_meetings("2026-08-19T12:01:00Z")
        .unwrap();

    assert_eq!(recovered, ["meeting-1"]);
    assert_eq!(
        repository.get_meeting("meeting-1").unwrap().unwrap().status,
        "interrupted"
    );
}

#[test]
fn minutes_reject_a_revision_older_than_the_latest_result() {
    let (_temp, repository) = test_repository();
    repository
        .create_meeting(&meeting_record("meeting-1", "产品周会"))
        .unwrap();

    repository
        .save_minutes("minutes-2", "meeting-1", 2, "新版纪要", "test-provider")
        .unwrap();
    let stale = repository.save_minutes("minutes-1", "meeting-1", 1, "旧版纪要", "test-provider");
    assert!(matches!(
        stale,
        Err(PersistenceError::StaleRevision {
            current: 2,
            attempted: 1
        })
    ));
    let latest: MeetingMinutesRow = repository.latest_minutes("meeting-1").unwrap().unwrap();
    assert_eq!(latest.revision, 2);
}

#[test]
fn processing_jobs_are_claimed_once_and_increment_attempts() {
    let (_temp, repository) = test_repository();
    repository
        .create_meeting(&meeting_record("meeting-1", "故障恢复"))
        .unwrap();
    repository
        .enqueue_processing_job(&NewProcessingJob {
            id: "job-1".to_string(),
            meeting_id: "meeting-1".to_string(),
            job_type: "file_transcription".to_string(),
            input_revision: Some(3),
            created_at: "2026-08-19T00:00:00Z".to_string(),
        })
        .unwrap();

    let claimed: ProcessingJobRow = repository
        .claim_next_processing_job("file_transcription", "2026-08-19T13:00:00Z")
        .unwrap()
        .expect("queued job");
    assert_eq!(claimed.id, "job-1");
    assert_eq!(claimed.status, "running");
    assert_eq!(claimed.attempts, 1);
    assert_eq!(claimed.input_revision, Some(3));
    assert!(repository
        .claim_next_processing_job("file_transcription", "2026-08-19T13:01:00Z")
        .unwrap()
        .is_none());
}

#[test]
fn permanent_delete_only_removes_a_trashed_meeting() {
    let (_temp, repository) = test_repository();
    repository
        .create_meeting(&meeting_record("meeting-1", "保留会议"))
        .unwrap();

    repository.permanently_delete_meeting("meeting-1").unwrap();
    assert_eq!(repository.list_meetings(false).unwrap().len(), 1);

    repository
        .soft_delete_meeting("meeting-1", "2026-08-19T12:00:00Z")
        .unwrap();
    repository.permanently_delete_meeting("meeting-1").unwrap();
    assert!(repository.list_meetings(true).unwrap().is_empty());
}

#[test]
fn recovery_parts_are_returned_in_run_order() {
    let temp = TempDir::new().expect("temporary directory");
    std::fs::write(temp.path().join("run-002.part"), b"second").unwrap();
    std::fs::write(temp.path().join("run-001.part"), b"first").unwrap();
    std::fs::write(temp.path().join("ignore.txt"), b"ignored").unwrap();

    let parts = aimeeting_lib::persistence::recovery::recovery_parts(temp.path()).unwrap();
    let names = parts
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(names, ["run-001.part", "run-002.part"]);
}

#[test]
fn data_paths_reject_identifiers_that_can_escape_the_root() {
    let temp = TempDir::new().expect("temporary directory");
    let paths = DataPaths::new(temp.path()).expect("data paths");

    assert!(paths.meeting_dir("../outside").is_err());
    assert!(paths.meeting_dir("meeting/child").is_err());
    assert!(paths.meeting_dir("meeting-合法_01").is_ok());
}

#[test]
fn trash_move_and_restore_preserve_recording_files() {
    let temp = TempDir::new().expect("temporary directory");
    let paths = DataPaths::new(temp.path()).expect("data paths");
    let meeting_dir = paths.meeting_dir("meeting-1").expect("meeting directory");
    std::fs::create_dir_all(&meeting_dir).unwrap();
    std::fs::write(meeting_dir.join("recording.ogg"), b"recording-data").unwrap();

    paths.move_to_trash("meeting-1").expect("move to trash");
    assert!(!meeting_dir.exists());
    assert_eq!(
        std::fs::read(paths.trash_dir("meeting-1").unwrap().join("recording.ogg")).unwrap(),
        b"recording-data"
    );

    paths
        .restore_from_trash("meeting-1")
        .expect("restore from trash");
    assert_eq!(
        std::fs::read(meeting_dir.join("recording.ogg")).unwrap(),
        b"recording-data"
    );
}

#[test]
fn permanent_trash_cleanup_removes_the_recording_directory() {
    let temp = TempDir::new().expect("temporary directory");
    let paths = DataPaths::new(temp.path()).expect("data paths");
    let meeting_dir = paths.meeting_dir("meeting-1").expect("meeting directory");
    std::fs::create_dir_all(&meeting_dir).unwrap();
    std::fs::write(meeting_dir.join("recording.opus"), b"recording-data").unwrap();
    paths.move_to_trash("meeting-1").unwrap();

    paths.permanently_delete_from_trash("meeting-1").unwrap();

    assert!(!paths.trash_dir("meeting-1").unwrap().exists());
}
