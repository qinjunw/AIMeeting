use aimeeting_lib::persistence::{import_legacy_meetings, LegacyMeetingImport, MeetingRepository};

#[test]
fn legacy_text_meetings_are_imported_once_without_fake_audio() {
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("aimeeting.db");
    let meetings = vec![LegacyMeetingImport {
        source_id: "meeting_old_1".to_string(),
        title: "旧版周会".to_string(),
        transcript: "确认周五发布。\n负责人是小李。".to_string(),
        minutes: "结论：周五发布。".to_string(),
        created_at: "2026-05-01T08:00:00Z".to_string(),
        updated_at: "2026-05-01T09:00:00Z".to_string(),
        stopped_at: Some("2026-05-01T09:00:00Z".to_string()),
    }];

    assert_eq!(import_legacy_meetings(&database, &meetings).unwrap(), 1);
    assert_eq!(import_legacy_meetings(&database, &meetings).unwrap(), 0);

    let repository = MeetingRepository::open(&database).unwrap();
    let imported = repository
        .get_meeting("legacy_meeting_old_1")
        .unwrap()
        .unwrap();
    assert_eq!(imported.title, "旧版周会");
    assert_eq!(imported.status, "ready");
    assert_eq!(
        repository.full_transcript("legacy_meeting_old_1").unwrap(),
        "确认周五发布。\n负责人是小李。"
    );
    assert_eq!(
        repository
            .latest_minutes("legacy_meeting_old_1")
            .unwrap()
            .unwrap()
            .content,
        "结论：周五发布。"
    );
    assert!(repository
        .latest_recording_asset("legacy_meeting_old_1")
        .unwrap()
        .is_none());
}
