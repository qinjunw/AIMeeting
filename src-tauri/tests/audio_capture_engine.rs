use std::sync::{Arc, Mutex};
use std::time::Duration;

use aimeeting_lib::audio::capture::{
    capture_channel, convert_samples_to_f32, AudioCaptureSource, AudioDeviceInfo,
    CaptureCoordinator, CaptureError, CaptureFrameSink, CaptureSourceKind, NativeSampleFormat,
    NativeSamples, SourceSelection,
};
use aimeeting_lib::audio::engine::{
    AudioEngine, AudioEngineConfig, AudioEngineError, CaptureHealthIssue,
};
use aimeeting_lib::audio::frame::{AudioFrame, AudioSource};
use aimeeting_lib::audio::mixer::MixerConfig;

fn frame(source: AudioSource, timestamp_ms: u64, samples: Vec<f32>) -> AudioFrame {
    AudioFrame::new(source, Duration::from_millis(timestamp_ms), 100, 1, samples)
        .expect("valid fixture")
}

fn device(kind: CaptureSourceKind, id: &str) -> AudioDeviceInfo {
    AudioDeviceInfo {
        id: id.to_string(),
        name: id.to_string(),
        kind,
        is_default: true,
        sample_format: NativeSampleFormat::F32,
        sample_rate: 100,
        channels: 1,
    }
}

#[derive(Default, Debug)]
struct FakeLifecycle {
    starts: usize,
    stops: usize,
}

struct FakeCaptureSource {
    info: AudioDeviceInfo,
    frames: Vec<AudioFrame>,
    lifecycle: Arc<Mutex<FakeLifecycle>>,
    running: bool,
}

impl FakeCaptureSource {
    fn new(
        kind: CaptureSourceKind,
        id: &str,
        frames: Vec<AudioFrame>,
    ) -> (Self, Arc<Mutex<FakeLifecycle>>) {
        let lifecycle = Arc::new(Mutex::new(FakeLifecycle::default()));
        (
            Self {
                info: device(kind, id),
                frames,
                lifecycle: Arc::clone(&lifecycle),
                running: false,
            },
            lifecycle,
        )
    }
}

impl AudioCaptureSource for FakeCaptureSource {
    fn info(&self) -> &AudioDeviceInfo {
        &self.info
    }

    fn start(&mut self, sink: CaptureFrameSink) -> Result<(), CaptureError> {
        if self.running {
            return Err(CaptureError::AlreadyRunning(self.info.id.clone()));
        }
        self.running = true;
        self.lifecycle.lock().expect("lifecycle").starts += 1;
        for frame in self.frames.clone() {
            sink.try_send(frame)?;
        }
        Ok(())
    }

    fn stop(&mut self) -> Result<(), CaptureError> {
        if self.running {
            self.running = false;
            self.lifecycle.lock().expect("lifecycle").stops += 1;
        }
        Ok(())
    }

    fn take_error(&self) -> Option<String> {
        None
    }
}

#[test]
fn coordinator_starts_and_stops_only_enabled_sources() {
    let (microphone, microphone_lifecycle) = FakeCaptureSource::new(
        CaptureSourceKind::Microphone,
        "microphone",
        vec![frame(AudioSource::Microphone, 0, vec![0.25, 0.25])],
    );
    let (system, system_lifecycle) = FakeCaptureSource::new(
        CaptureSourceKind::System,
        "system",
        vec![frame(AudioSource::System, 0, vec![0.5, 0.5])],
    );
    let (sink, receiver) = capture_channel(4).expect("capture channel");
    let mut coordinator =
        CaptureCoordinator::new(Some(Box::new(microphone)), Some(Box::new(system)));

    coordinator
        .start(
            SourceSelection {
                microphone: true,
                system: false,
            },
            sink,
        )
        .expect("start microphone");
    coordinator.stop().expect("stop capture");

    assert_eq!(microphone_lifecycle.lock().unwrap().starts, 1);
    assert_eq!(microphone_lifecycle.lock().unwrap().stops, 1);
    assert_eq!(system_lifecycle.lock().unwrap().starts, 0);
    assert_eq!(system_lifecycle.lock().unwrap().stops, 0);
    assert_eq!(receiver.try_iter().count(), 1);
}

#[test]
fn native_f32_i16_and_u16_samples_convert_to_normalized_f32() {
    let f32_samples = convert_samples_to_f32(NativeSamples::F32(&[-1.0, 0.25, 1.0]));
    let i16_samples = convert_samples_to_f32(NativeSamples::I16(&[i16::MIN, 0, i16::MAX]));
    let u16_samples = convert_samples_to_f32(NativeSamples::U16(&[0, 32_768, u16::MAX]));

    assert_eq!(f32_samples, vec![-1.0, 0.25, 1.0]);
    assert_eq!(i16_samples[0], -1.0);
    assert_eq!(i16_samples[1], 0.0);
    assert!((i16_samples[2] - 0.999_969_5).abs() < 0.000_001);
    assert_eq!(u16_samples[0], -1.0);
    assert_eq!(u16_samples[1], 0.0);
    assert!((u16_samples[2] - 0.999_969_5).abs() < 0.000_001);
}

fn engine(selection: SourceSelection, recorder_samples: usize, asr_samples: usize) -> AudioEngine {
    AudioEngine::new(
        AudioEngineConfig {
            output_sample_rate: 100,
            output_frame_samples: 2,
            recorder_capacity_samples: recorder_samples,
            asr_capacity_samples: asr_samples,
            alignment_latency: Duration::ZERO,
            silence_rms_threshold: 0.001,
            silence_warning_after: Duration::from_millis(40),
            mixer: MixerConfig {
                microphone_gain: 1.0,
                system_gain: 1.0,
                limiter_threshold: 1.0,
            },
        },
        selection,
    )
    .expect("engine")
}

#[test]
fn engine_supports_microphone_system_and_mixed_source_combinations() {
    let microphone_frame = frame(AudioSource::Microphone, 0, vec![0.75, 0.25]);
    let system_frame = frame(AudioSource::System, 0, vec![0.5, 0.5]);

    let mut microphone_only = engine(SourceSelection::microphone_only(), 2, 2);
    microphone_only
        .ingest(microphone_frame.clone())
        .expect("microphone");
    microphone_only
        .ingest(system_frame.clone())
        .expect("disabled system ignored");
    microphone_only
        .advance_to(Duration::from_millis(20))
        .expect("advance microphone timeline");
    assert_eq!(
        microphone_only.pop_recorder().unwrap().samples(),
        &[0.75, 0.25]
    );
    assert!(microphone_only.pop_recorder().is_none());

    let mut system_only = engine(SourceSelection::system_only(), 2, 2);
    system_only.ingest(system_frame.clone()).expect("system");
    system_only
        .advance_to(Duration::from_millis(20))
        .expect("advance system timeline");
    assert_eq!(system_only.pop_recorder().unwrap().samples(), &[0.5, 0.5]);

    let mut mixed = engine(SourceSelection::mixed(), 2, 2);
    mixed.ingest(microphone_frame).expect("buffer microphone");
    assert!(mixed.pop_recorder().is_none());
    mixed.ingest(system_frame).expect("mix system");
    mixed
        .advance_to(Duration::from_millis(20))
        .expect("advance mixed timeline");
    assert_eq!(mixed.pop_recorder().unwrap().samples(), &[1.0, 0.75]);
}

#[test]
fn mixed_timeline_keeps_recording_when_system_loopback_is_idle() {
    let mut engine = engine(SourceSelection::mixed(), 8, 8);
    engine
        .ingest(frame(AudioSource::Microphone, 0, vec![0.4, 0.2]))
        .expect("microphone frame");

    engine
        .advance_to(Duration::from_millis(40))
        .expect("advance through missing system audio");

    assert_eq!(engine.pop_recorder().unwrap().samples(), &[0.4, 0.2]);
    assert_eq!(engine.pop_recorder().unwrap().samples(), &[0.0, 0.0]);
    assert!(engine.pop_recorder().is_none());
}

#[test]
fn delayed_system_audio_is_aligned_to_its_wall_clock_slot() {
    let mut engine = engine(SourceSelection::mixed(), 12, 12);
    for (timestamp, samples) in [
        (0, vec![0.1, 0.1]),
        (20, vec![0.2, 0.2]),
        (40, vec![0.3, 0.3]),
    ] {
        engine
            .ingest(frame(AudioSource::Microphone, timestamp, samples))
            .expect("microphone frame");
    }
    engine
        .ingest(frame(AudioSource::System, 40, vec![0.5, 0.5]))
        .expect("delayed system frame");

    engine
        .advance_to(Duration::from_millis(60))
        .expect("advance aligned timeline");

    assert_eq!(engine.pop_recorder().unwrap().samples(), &[0.1, 0.1]);
    assert_eq!(engine.pop_recorder().unwrap().samples(), &[0.2, 0.2]);
    assert_eq!(engine.pop_recorder().unwrap().samples(), &[0.8, 0.8]);
}

#[test]
fn system_only_timeline_emits_duration_preserving_silence_without_callbacks() {
    let mut engine = engine(SourceSelection::system_only(), 6, 6);

    engine
        .advance_to(Duration::from_millis(40))
        .expect("advance silent system timeline");

    assert_eq!(engine.pop_recorder().unwrap().samples(), &[0.0, 0.0]);
    assert_eq!(engine.pop_recorder().unwrap().samples(), &[0.0, 0.0]);
    assert!(engine.pop_recorder().is_none());
}

#[test]
fn system_loopback_resumes_in_its_wall_clock_slot_after_a_silent_gap() {
    let mut engine = engine(SourceSelection::system_only(), 12, 12);
    engine
        .ingest(frame(AudioSource::System, 0, vec![0.5, 0.5]))
        .expect("first playback");
    engine
        .advance_to(Duration::from_millis(100))
        .expect("fill silent gap");
    engine
        .ingest(frame(AudioSource::System, 100, vec![0.7, 0.7]))
        .expect("resumed playback");
    engine
        .advance_to(Duration::from_millis(120))
        .expect("emit resumed playback");

    assert_eq!(engine.pop_recorder().unwrap().samples(), &[0.5, 0.5]);
    for _ in 0..4 {
        assert_eq!(engine.pop_recorder().unwrap().samples(), &[0.0, 0.0]);
    }
    assert_eq!(engine.pop_recorder().unwrap().samples(), &[0.7, 0.7]);
}

#[test]
fn asr_saturation_drops_only_asr_frames_and_preserves_recorder_frames() {
    let mut engine = engine(SourceSelection::microphone_only(), 4, 2);

    engine
        .ingest(frame(AudioSource::Microphone, 0, vec![0.1, 0.2]))
        .expect("first frame");
    engine
        .advance_to(Duration::from_millis(20))
        .expect("fill recorder queue");
    engine
        .ingest(frame(AudioSource::Microphone, 20, vec![0.3, 0.4]))
        .expect("ASR congestion must not fail recording");
    engine
        .advance_to(Duration::from_millis(40))
        .expect("ASR congestion must not fail recording");

    assert_eq!(engine.pop_recorder().unwrap().samples(), &[0.1, 0.2]);
    assert_eq!(engine.pop_recorder().unwrap().samples(), &[0.3, 0.4]);
    assert!(engine.pop_recorder().is_none());
    assert_eq!(engine.pop_asr().unwrap().samples(), &[0.1, 0.2]);
    assert!(engine.pop_asr().is_none());
    assert_eq!(engine.metrics().asr_dropped_frames, 1);
    assert!(engine.metrics().asr_degraded);
    assert_eq!(engine.metrics().recorder_failures, 0);
}

#[test]
fn recorder_saturation_is_an_explicit_fatal_engine_error() {
    let mut engine = engine(SourceSelection::microphone_only(), 2, 4);
    engine
        .ingest(frame(AudioSource::Microphone, 0, vec![0.1, 0.2]))
        .expect("first frame");
    engine
        .advance_to(Duration::from_millis(20))
        .expect("fill recorder queue");

    engine
        .ingest(frame(AudioSource::Microphone, 20, vec![0.3, 0.4]))
        .expect("buffer second frame");
    let error = engine
        .advance_to(Duration::from_millis(40))
        .expect_err("recorder congestion must be explicit");

    assert!(matches!(error, AudioEngineError::RecorderQueueFull { .. }));
    assert_eq!(engine.metrics().recorder_failures, 1);
    assert_eq!(engine.pop_recorder().unwrap().samples(), &[0.1, 0.2]);
    assert!(engine.pop_recorder().is_none());
}

#[test]
fn sustained_near_zero_rms_marks_the_source_as_degraded() {
    let mut engine = engine(SourceSelection::microphone_only(), 6, 6);

    for timestamp in [0, 20, 40] {
        engine
            .ingest(frame(
                AudioSource::Microphone,
                timestamp,
                vec![0.000_01, -0.000_01],
            ))
            .expect("silent frame");
    }

    assert!(engine.health_warnings().iter().any(|warning| {
        warning.source == CaptureSourceKind::Microphone
            && warning.issue == CaptureHealthIssue::NearZeroSignal
            && warning.observed_for >= Duration::from_millis(40)
    }));
}
