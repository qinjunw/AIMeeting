use std::sync::Arc;
use std::time::Duration;

use aimeeting_lib::audio::capture::{
    AudioCaptureSource, AudioDeviceInfo, CaptureError, CaptureFrameSink, CaptureSourceKind,
    NativeSampleFormat, SourceSelection,
};
use aimeeting_lib::audio::frame::{AudioFrame, AudioSource};
use aimeeting_lib::audio::ogg_opus::scan_ogg_file;
use aimeeting_lib::runtime::registry::{
    CaptureSourceFactory, RecordingRegistry, RuntimeRecordingStatus,
};

struct FakeFactory;

impl CaptureSourceFactory for FakeFactory {
    fn create(&self, kind: CaptureSourceKind) -> Result<Box<dyn AudioCaptureSource>, String> {
        Ok(Box::new(FakeSource::new(kind)))
    }
}

struct FakeSource {
    info: AudioDeviceInfo,
}

impl FakeSource {
    fn new(kind: CaptureSourceKind) -> Self {
        Self {
            info: AudioDeviceInfo {
                id: format!("fake-{kind:?}"),
                name: format!("Fake {kind:?}"),
                kind,
                is_default: true,
                sample_format: NativeSampleFormat::F32,
                sample_rate: 48_000,
                channels: 1,
            },
        }
    }
}

impl AudioCaptureSource for FakeSource {
    fn info(&self) -> &AudioDeviceInfo {
        &self.info
    }

    fn start(&mut self, sink: CaptureFrameSink) -> Result<(), CaptureError> {
        let source = match self.info.kind {
            CaptureSourceKind::Microphone => AudioSource::Microphone,
            CaptureSourceKind::System => AudioSource::System,
        };
        sink.try_send(
            AudioFrame::new(source, Duration::ZERO, 48_000, 1, vec![0.05; 960])
                .expect("fake frame"),
        )
    }

    fn stop(&mut self) -> Result<(), CaptureError> {
        Ok(())
    }

    fn take_error(&self) -> Option<String> {
        None
    }
}

fn registry() -> RecordingRegistry {
    RecordingRegistry::new(Arc::new(FakeFactory))
}

#[test]
fn pause_and_resume_create_recoverable_chained_ogg_runs() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("recording.opus");
    let mut registry = registry();

    let started = registry
        .start(
            "meeting-1".to_string(),
            1,
            SourceSelection::microphone_only(),
            path.clone(),
        )
        .expect("start");
    assert_eq!(started.status, RuntimeRecordingStatus::Recording);
    std::thread::sleep(Duration::from_millis(70));

    let paused = registry.pause("meeting-1").expect("pause");
    assert_eq!(paused.status, RuntimeRecordingStatus::Paused);
    let resumed = registry
        .resume("meeting-1", 2, SourceSelection::mixed())
        .expect("resume");
    assert_eq!(resumed.generation, 2);
    std::thread::sleep(Duration::from_millis(70));

    let checkpoint = registry.stop("meeting-1").expect("stop");
    assert_eq!(checkpoint.completed_runs, 2);
    assert!(checkpoint.recorded_samples >= 4_800);
    assert!(registry.active().is_none());

    let scan = scan_ogg_file(&path).expect("scan");
    assert_eq!(scan.streams.len(), 2);
    assert!(scan.streams.iter().all(|stream| stream.has_eos));
    assert_eq!(scan.total_duration_samples(), checkpoint.recorded_samples);
}

#[test]
fn a_new_meeting_can_start_immediately_after_local_stop_finishes() {
    let directory = tempfile::tempdir().expect("tempdir");
    let mut registry = registry();

    for (index, meeting_id) in ["meeting-1", "meeting-2"].into_iter().enumerate() {
        registry
            .start(
                meeting_id.to_string(),
                1,
                SourceSelection::microphone_only(),
                directory.path().join(format!("recording-{index}.opus")),
            )
            .expect("start meeting");
        std::thread::sleep(Duration::from_millis(30));
        registry.stop(meeting_id).expect("stop meeting");
    }

    assert!(registry.active().is_none());
}

#[test]
fn registry_rejects_a_second_active_meeting_without_touching_the_first() {
    let directory = tempfile::tempdir().expect("tempdir");
    let mut registry = registry();
    registry
        .start(
            "meeting-1".to_string(),
            1,
            SourceSelection::microphone_only(),
            directory.path().join("first.opus"),
        )
        .expect("start first");

    let error = registry
        .start(
            "meeting-2".to_string(),
            1,
            SourceSelection::microphone_only(),
            directory.path().join("second.opus"),
        )
        .expect_err("second start must fail");

    assert!(error.contains("已有会议"));
    assert_eq!(registry.active().unwrap().meeting_id, "meeting-1");
    registry.stop("meeting-1").expect("cleanup");
}
