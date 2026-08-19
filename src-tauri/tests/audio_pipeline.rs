#[path = "../src/audio/mod.rs"]
mod audio;

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::time::Duration;

use audio::fake::SyntheticAudioSource;
use audio::frame::{AudioFrame, AudioSource};
use audio::mixer::{AudioMixer, BoundedFrameQueue, MixerConfig, QueueError};
use audio::ogg_opus::{recover_truncated_file, scan_ogg_file, OggOpusConfig, OggOpusWriter};
use audio::preprocessor::measure_level;
use audio::resampler::{resample_linear, StreamingLinearResampler};
use ogg::PacketReader;
use tempfile::TempDir;

fn frame(
    source: AudioSource,
    timestamp_ms: u64,
    sample_rate: u32,
    channels: u16,
    samples: Vec<f32>,
) -> AudioFrame {
    AudioFrame::new(
        source,
        Duration::from_millis(timestamp_ms),
        sample_rate,
        channels,
        samples,
    )
    .expect("valid test frame")
}

fn sine(sample_count: usize, sample_rate: u32, frequency_hz: f32) -> Vec<f32> {
    (0..sample_count)
        .map(|index| {
            let phase = index as f32 * frequency_hz * std::f32::consts::TAU / sample_rate as f32;
            phase.sin() * 0.25
        })
        .collect()
}

#[test]
fn downmixes_interleaved_stereo_to_mono_without_changing_timeline() {
    let stereo = frame(
        AudioSource::Microphone,
        125,
        48_000,
        2,
        vec![1.0, -1.0, 0.5, 0.25, -0.25, -0.75],
    );

    let mono = stereo.to_mono().expect("downmix");

    assert_eq!(mono.channels(), 1);
    assert_eq!(mono.sample_rate(), 48_000);
    assert_eq!(mono.timestamp(), Duration::from_millis(125));
    assert_eq!(mono.samples(), &[0.0, 0.375, -0.5]);
}

#[test]
fn linear_resampling_preserves_endpoints_and_expected_point_count() {
    let output = resample_linear(&[0.0, 1.0, 2.0, 3.0], 4, 8).expect("resample");

    assert_eq!(output.len(), 7);
    assert_eq!(output.first(), Some(&0.0));
    assert_eq!(output.last(), Some(&3.0));
    assert_eq!(output, vec![0.0, 0.5, 1.0, 1.5, 2.0, 2.5, 3.0]);
}

#[test]
fn mixer_applies_independent_gains_and_limits_the_combined_signal() {
    let microphone = frame(AudioSource::Microphone, 20, 48_000, 1, vec![0.8, -0.8, 0.2]);
    let system = frame(AudioSource::System, 20, 48_000, 1, vec![0.8, -0.8, -0.2]);
    let mut mixer = AudioMixer::new(MixerConfig {
        microphone_gain: 1.0,
        system_gain: 0.5,
        limiter_threshold: 0.9,
    })
    .expect("mixer");

    let mixed = mixer
        .mix(Some(&microphone), Some(&system))
        .expect("mix frames");

    assert_eq!(mixed.source(), AudioSource::Mixed);
    assert_eq!(mixed.samples(), &[0.9, -0.9, 0.1]);
}

#[test]
fn silence_level_reports_zero_rms_and_zero_peak() {
    let level = measure_level(&[0.0; 960], 0.001);

    assert_eq!(level.rms, 0.0);
    assert_eq!(level.peak, 0.0);
    assert!(level.is_silent);

    let active = measure_level(&[1.0, -1.0], 0.001);
    assert_eq!(active.rms, 1.0);
    assert_eq!(active.peak, 1.0);
    assert!(!active.is_silent);
}

#[test]
fn bounded_queue_rejects_timestamp_regression_even_after_a_pop() {
    let mut queue = BoundedFrameQueue::new(4).expect("queue");
    queue
        .push(frame(AudioSource::Microphone, 20, 100, 1, vec![0.0, 0.0]))
        .expect("first frame");
    queue.pop().expect("queued frame");

    let error = queue
        .push(frame(AudioSource::Microphone, 10, 100, 1, vec![0.0, 0.0]))
        .expect_err("older timestamp must fail");

    assert!(matches!(error, QueueError::TimestampRegression { .. }));
}

#[test]
fn bounded_queue_reports_capacity_without_dropping_existing_audio() {
    let mut queue = BoundedFrameQueue::new(3).expect("queue");
    queue
        .push(frame(AudioSource::System, 0, 100, 1, vec![0.1, 0.2]))
        .expect("first frame");

    let error = queue
        .push(frame(AudioSource::System, 20, 100, 1, vec![0.3, 0.4]))
        .expect_err("queue must not silently drop audio");

    assert!(matches!(
        error,
        QueueError::CapacityExceeded {
            capacity_samples: 3,
            attempted_samples: 4
        }
    ));
    assert_eq!(queue.buffered_samples(), 2);
    assert_eq!(queue.pop().expect("original frame").samples(), &[0.1, 0.2]);
}

#[test]
fn accelerated_thirty_minute_source_keeps_sample_count_and_buffer_bounded() {
    // 5,001 Hz models a +200 ppm source clock against a 5,000 Hz mix timeline.
    let mut source = SyntheticAudioSource::silence(
        AudioSource::Microphone,
        5_001,
        1,
        Duration::from_millis(20),
        Duration::from_secs(30 * 60),
    )
    .expect("synthetic source");
    let mut resampler = StreamingLinearResampler::new(5_001, 5_000).expect("resampler");
    let mut queue = BoundedFrameQueue::new(202).expect("queue");
    let mut output_samples = 0usize;

    while let Some(frame) = source.next_frame() {
        queue.push(frame).expect("bounded producer");
        let frame = queue.pop().expect("bounded consumer");
        output_samples += resampler.process(frame.samples()).len();
    }

    assert_eq!(source.generated_samples(), 9_001_800);
    assert_eq!(output_samples, 9_000_000);
    assert!(queue.high_watermark_samples() <= 101);
    assert!(resampler.buffered_samples() <= 1);
}

#[test]
fn finalized_ogg_opus_run_has_headers_eos_and_exact_trimmed_duration() {
    let temp = TempDir::new().expect("temp directory");
    let path = temp.path().join("single-run.opus");
    let mut writer = OggOpusWriter::create(&path, OggOpusConfig::default()).expect("writer");
    let samples = sine(4_923, 48_000, 440.0);

    writer.begin_run(101).expect("begin run");
    writer.write_pcm(&samples).expect("write pcm");
    let run = writer.finish_run().expect("finish run");
    writer.finalize().expect("finalize file");

    assert_eq!(run.input_samples, 4_923);
    assert_eq!(run.packet_count, 6);

    let scan = scan_ogg_file(&path).expect("scan ogg");
    assert_eq!(scan.streams.len(), 1);
    assert_eq!(scan.streams[0].serial, 101);
    assert!(scan.streams[0].has_bos);
    assert!(scan.streams[0].has_eos);
    assert_eq!(scan.total_duration_samples(), 4_923);

    let packets = read_packets(&path);
    assert!(packets[0].2.starts_with(b"OpusHead"));
    assert!(packets[1].2.starts_with(b"OpusTags"));
    assert!(packets.last().expect("last packet").1);
}

#[test]
fn pause_resume_writes_two_valid_logical_streams_into_one_chained_file() {
    let temp = TempDir::new().expect("temp directory");
    let path = temp.path().join("chained.opus");
    let mut writer = OggOpusWriter::create(&path, OggOpusConfig::default()).expect("writer");

    writer.begin_run(11).expect("first run");
    writer
        .write_pcm(&sine(2_880, 48_000, 220.0))
        .expect("first pcm");
    writer.finish_run().expect("finish first run");

    writer.begin_run(22).expect("second run");
    writer
        .write_pcm(&sine(1_920, 48_000, 330.0))
        .expect("second pcm");
    writer.finish_run().expect("finish second run");
    writer.finalize().expect("finalize file");

    let scan = scan_ogg_file(&path).expect("scan ogg");
    assert_eq!(scan.streams.len(), 2);
    assert_eq!(scan.streams[0].serial, 11);
    assert_eq!(scan.streams[1].serial, 22);
    assert!(scan.streams.iter().all(|stream| stream.has_eos));
    assert_eq!(scan.total_duration_samples(), 4_800);

    let heads: Vec<u32> = read_packets(&path)
        .into_iter()
        .filter_map(|(serial, _, data)| data.starts_with(b"OpusHead").then_some(serial))
        .collect();
    assert_eq!(heads, vec![11, 22]);
}

#[test]
fn configured_pre_skip_is_used_consistently_in_headers_and_granules() {
    let temp = TempDir::new().expect("temp directory");
    let path = temp.path().join("custom-pre-skip.opus");
    let config = OggOpusConfig {
        pre_skip: 80,
        ..OggOpusConfig::default()
    };
    let mut writer = OggOpusWriter::create(&path, config).expect("writer");

    writer.begin_run(33).expect("begin run");
    writer
        .write_pcm(&sine(960, 48_000, 440.0))
        .expect("write pcm");
    writer.finish_run().expect("finish run");
    writer.finalize().expect("finalize file");

    let scan = scan_ogg_file(&path).expect("scan ogg");
    assert_eq!(scan.streams[0].pre_skip, 80);
    assert_eq!(scan.total_duration_samples(), 960);
}

#[test]
fn recovery_truncates_partial_page_and_marks_last_complete_page_as_eos() {
    let temp = TempDir::new().expect("temp directory");
    let path = temp.path().join("interrupted.opus");
    let config = OggOpusConfig {
        packets_per_page: 1,
        ..OggOpusConfig::default()
    };
    let mut writer = OggOpusWriter::create(&path, config).expect("writer");
    writer.begin_run(77).expect("begin run");
    writer
        .write_pcm(&sine(2_880, 48_000, 440.0))
        .expect("write pcm");
    drop(writer);

    let before_corruption = scan_ogg_file(&path).expect("scan checkpointed pages");
    assert!(!before_corruption.streams[0].has_eos);
    let complete_len = before_corruption.complete_len;

    let mut file = OpenOptions::new()
        .append(true)
        .open(&path)
        .expect("append partial page");
    file.write_all(b"OggS\0\0partial-page")
        .expect("write partial page");
    file.sync_data().expect("sync corruption fixture");
    drop(file);

    let recovered = recover_truncated_file(&path).expect("recover file");
    assert_eq!(recovered.complete_len, complete_len);
    assert_eq!(
        File::open(&path)
            .expect("recovered file")
            .metadata()
            .unwrap()
            .len(),
        complete_len
    );
    assert!(recovered.streams[0].has_eos);
    assert_eq!(recovered.total_duration_samples(), 1_920);

    let packets = read_packets(&path);
    assert!(packets.last().expect("last recovered packet").1);
}

fn read_packets(path: &std::path::Path) -> Vec<(u32, bool, Vec<u8>)> {
    let file = File::open(path).expect("open ogg file");
    let mut reader = PacketReader::new(file);
    let mut packets = Vec::new();
    while let Some(packet) = reader.read_packet().expect("valid Ogg packet") {
        packets.push((packet.stream_serial(), packet.last_in_stream(), packet.data));
    }
    packets
}
