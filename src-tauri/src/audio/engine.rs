use std::collections::{BTreeMap, VecDeque};
use std::time::Duration;

use thiserror::Error;

use super::capture::{CaptureSourceKind, SourceSelection};
use super::frame::{AudioFrame, AudioSource, FrameError};
use super::mixer::{AudioMixer, BoundedFrameQueue, MixerConfig, MixerError, QueueError};
use super::preprocessor::measure_level;
use super::resampler::{ResampleError, StreamingLinearResampler};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AudioEngineConfig {
    pub output_sample_rate: u32,
    pub output_frame_samples: usize,
    pub recorder_capacity_samples: usize,
    pub asr_capacity_samples: usize,
    pub alignment_latency: Duration,
    pub silence_rms_threshold: f32,
    pub silence_warning_after: Duration,
    pub mixer: MixerConfig,
}

impl Default for AudioEngineConfig {
    fn default() -> Self {
        Self {
            output_sample_rate: 48_000,
            output_frame_samples: 960,
            recorder_capacity_samples: 48_000 * 5,
            asr_capacity_samples: 48_000,
            alignment_latency: Duration::from_millis(100),
            silence_rms_threshold: 0.000_1,
            silence_warning_after: Duration::from_secs(3),
            mixer: MixerConfig {
                microphone_gain: 1.0,
                system_gain: 1.0,
                limiter_threshold: 0.98,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AudioEngineMetrics {
    pub recorder_failures: u64,
    pub asr_dropped_frames: u64,
    pub asr_degraded: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureHealthIssue {
    NearZeroSignal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureHealthWarning {
    pub source: CaptureSourceKind,
    pub issue: CaptureHealthIssue,
    pub observed_for: Duration,
}

pub struct AudioEngine {
    config: AudioEngineConfig,
    selection: SourceSelection,
    mixer: AudioMixer,
    microphone: SourceNormalizer,
    system: SourceNormalizer,
    pending_microphone: BTreeMap<Duration, AudioFrame>,
    pending_system: BTreeMap<Duration, AudioFrame>,
    next_output_samples: u64,
    recorder: BoundedFrameQueue,
    asr: BoundedFrameQueue,
    microphone_silent_samples: u64,
    system_silent_samples: u64,
    metrics: AudioEngineMetrics,
}

impl AudioEngine {
    pub fn new(
        config: AudioEngineConfig,
        selection: SourceSelection,
    ) -> Result<Self, AudioEngineError> {
        if !selection.microphone && !selection.system {
            return Err(AudioEngineError::NoSourcesSelected);
        }
        if config.output_sample_rate == 0 || config.output_frame_samples == 0 {
            return Err(AudioEngineError::InvalidOutputFormat);
        }
        if !config.silence_rms_threshold.is_finite()
            || config.silence_rms_threshold < 0.0
            || config.silence_warning_after.is_zero()
        {
            return Err(AudioEngineError::InvalidHealthConfiguration);
        }

        Ok(Self {
            mixer: AudioMixer::new(config.mixer)?,
            microphone: SourceNormalizer::new(
                AudioSource::Microphone,
                config.output_sample_rate,
                config.output_frame_samples,
            ),
            system: SourceNormalizer::new(
                AudioSource::System,
                config.output_sample_rate,
                config.output_frame_samples,
            ),
            pending_microphone: BTreeMap::new(),
            pending_system: BTreeMap::new(),
            next_output_samples: 0,
            recorder: BoundedFrameQueue::new(config.recorder_capacity_samples)?,
            asr: BoundedFrameQueue::new(config.asr_capacity_samples)?,
            config,
            selection,
            microphone_silent_samples: 0,
            system_silent_samples: 0,
            metrics: AudioEngineMetrics::default(),
        })
    }

    pub fn ingest(&mut self, frame: AudioFrame) -> Result<(), AudioEngineError> {
        let kind = match frame.source() {
            AudioSource::Microphone => CaptureSourceKind::Microphone,
            AudioSource::System => CaptureSourceKind::System,
            AudioSource::Mixed => return Err(AudioEngineError::MixedFrameAsInput),
        };
        if !self.selection.includes(kind) {
            return Ok(());
        }

        let normalized = match kind {
            CaptureSourceKind::Microphone => self.microphone.push(frame)?,
            CaptureSourceKind::System => self.system.push(frame)?,
        };
        for frame in normalized {
            self.observe_health(kind, &frame);
            self.buffer_source(kind, frame)?;
        }
        Ok(())
    }

    pub fn advance_to(&mut self, elapsed: Duration) -> Result<(), AudioEngineError> {
        self.emit_until(elapsed.saturating_sub(self.config.alignment_latency))
    }

    pub fn flush_to(&mut self, elapsed: Duration) -> Result<(), AudioEngineError> {
        self.emit_until(elapsed)
    }

    pub fn pop_recorder(&mut self) -> Option<AudioFrame> {
        self.recorder.pop()
    }

    pub fn pop_asr(&mut self) -> Option<AudioFrame> {
        self.asr.pop()
    }

    pub fn metrics(&self) -> AudioEngineMetrics {
        self.metrics
    }

    pub fn health_warnings(&self) -> Vec<CaptureHealthWarning> {
        let mut warnings = Vec::new();
        for (source, samples) in [
            (
                CaptureSourceKind::Microphone,
                self.microphone_silent_samples,
            ),
            (CaptureSourceKind::System, self.system_silent_samples),
        ] {
            if self.selection.includes(source) {
                let observed_for = duration_for_samples(samples, self.config.output_sample_rate);
                if observed_for >= self.config.silence_warning_after {
                    warnings.push(CaptureHealthWarning {
                        source,
                        issue: CaptureHealthIssue::NearZeroSignal,
                        observed_for,
                    });
                }
            }
        }
        warnings
    }

    fn observe_health(&mut self, kind: CaptureSourceKind, frame: &AudioFrame) {
        let level = measure_level(frame.samples(), self.config.silence_rms_threshold);
        let counter = match kind {
            CaptureSourceKind::Microphone => &mut self.microphone_silent_samples,
            CaptureSourceKind::System => &mut self.system_silent_samples,
        };
        if level.is_silent {
            *counter += frame.sample_frames() as u64;
        } else {
            *counter = 0;
        }
    }

    fn buffer_source(
        &mut self,
        kind: CaptureSourceKind,
        frame: AudioFrame,
    ) -> Result<(), AudioEngineError> {
        let timestamp = frame.timestamp();
        let next_timestamp =
            duration_for_samples(self.next_output_samples, self.config.output_sample_rate);
        if timestamp < next_timestamp {
            return Ok(());
        }
        match kind {
            CaptureSourceKind::Microphone => {
                self.pending_microphone.insert(timestamp, frame);
            }
            CaptureSourceKind::System => {
                self.pending_system.insert(timestamp, frame);
            }
        }

        let pending_samples = self.pending_microphone.len().max(self.pending_system.len())
            * self.config.output_frame_samples;
        if pending_samples > self.config.recorder_capacity_samples {
            return Err(AudioEngineError::SourceAlignmentQueueFull {
                capacity_samples: self.config.recorder_capacity_samples,
                attempted_samples: pending_samples,
            });
        }
        Ok(())
    }

    fn emit_until(&mut self, horizon: Duration) -> Result<(), AudioEngineError> {
        let frame_duration = duration_for_samples(
            self.config.output_frame_samples as u64,
            self.config.output_sample_rate,
        );
        loop {
            let timestamp =
                duration_for_samples(self.next_output_samples, self.config.output_sample_rate);
            if timestamp.saturating_add(frame_duration) > horizon {
                break;
            }

            let microphone = self.pending_microphone.remove(&timestamp);
            let system = self.pending_system.remove(&timestamp);
            let mixed = match (microphone.as_ref(), system.as_ref()) {
                (None, None) => AudioFrame::new(
                    AudioSource::Mixed,
                    timestamp,
                    self.config.output_sample_rate,
                    1,
                    vec![0.0; self.config.output_frame_samples],
                )?,
                (microphone, system) => self.mixer.mix(microphone, system)?,
            };
            self.dispatch(mixed)?;
            self.next_output_samples += self.config.output_frame_samples as u64;
        }
        Ok(())
    }

    fn dispatch(&mut self, frame: AudioFrame) -> Result<(), AudioEngineError> {
        if let Err(error) = self.recorder.push(frame.clone()) {
            self.metrics.recorder_failures += 1;
            return match error {
                QueueError::CapacityExceeded {
                    capacity_samples,
                    attempted_samples,
                } => Err(AudioEngineError::RecorderQueueFull {
                    capacity_samples,
                    attempted_samples,
                }),
                other => Err(AudioEngineError::RecorderQueueRejected(other)),
            };
        }

        if self.asr.push(frame).is_err() {
            self.metrics.asr_dropped_frames += 1;
            self.metrics.asr_degraded = true;
        }
        Ok(())
    }
}

struct SourceNormalizer {
    source: AudioSource,
    output_sample_rate: u32,
    output_frame_samples: usize,
    input_sample_rate: Option<u32>,
    resampler: Option<StreamingLinearResampler>,
    buffered: VecDeque<f32>,
    output_samples: u64,
    expected_input_end: Option<Duration>,
}

impl SourceNormalizer {
    fn new(source: AudioSource, output_sample_rate: u32, output_frame_samples: usize) -> Self {
        Self {
            source,
            output_sample_rate,
            output_frame_samples,
            input_sample_rate: None,
            resampler: None,
            buffered: VecDeque::new(),
            output_samples: 0,
            expected_input_end: None,
        }
    }

    fn push(&mut self, frame: AudioFrame) -> Result<Vec<AudioFrame>, AudioEngineError> {
        let frame = frame.to_mono()?;
        if self
            .input_sample_rate
            .is_some_and(|rate| rate != frame.sample_rate())
        {
            return Err(AudioEngineError::InputSampleRateChanged {
                audio_source: self.source,
                previous: self.input_sample_rate.expect("checked above"),
                incoming: frame.sample_rate(),
            });
        }
        let input_duration =
            duration_for_samples(frame.sample_frames() as u64, frame.sample_rate());
        let discontinuity_tolerance = duration_for_samples(
            (self.output_frame_samples * 2) as u64,
            self.output_sample_rate,
        );
        let is_discontinuous = self.expected_input_end.is_some_and(|expected| {
            frame.timestamp() > expected.saturating_add(discontinuity_tolerance)
        });
        if self.resampler.is_none() || is_discontinuous {
            self.input_sample_rate = Some(frame.sample_rate());
            self.resampler = Some(StreamingLinearResampler::new(
                frame.sample_rate(),
                self.output_sample_rate,
            )?);
            self.buffered.clear();
            self.output_samples = snap_to_frame_grid(
                samples_for_duration(frame.timestamp(), self.output_sample_rate),
                self.output_frame_samples,
            );
        }
        self.expected_input_end = Some(frame.timestamp().saturating_add(input_duration));
        let samples = self
            .resampler
            .as_mut()
            .expect("resampler was initialized")
            .process(frame.samples());
        self.buffered.extend(samples);

        let mut output = Vec::new();
        while self.buffered.len() >= self.output_frame_samples {
            let samples: Vec<f32> = self.buffered.drain(..self.output_frame_samples).collect();
            let timestamp = duration_for_samples(self.output_samples, self.output_sample_rate);
            self.output_samples += self.output_frame_samples as u64;
            output.push(AudioFrame::new(
                self.source,
                timestamp,
                self.output_sample_rate,
                1,
                samples,
            )?);
        }
        Ok(output)
    }
}

fn duration_for_samples(samples: u64, sample_rate: u32) -> Duration {
    Duration::from_secs_f64(samples as f64 / sample_rate as f64)
}

fn samples_for_duration(duration: Duration, sample_rate: u32) -> u64 {
    (duration.as_secs_f64() * sample_rate as f64).round() as u64
}

fn snap_to_frame_grid(samples: u64, frame_samples: usize) -> u64 {
    let frame_samples = frame_samples as u64;
    ((samples + frame_samples / 2) / frame_samples) * frame_samples
}

#[derive(Debug, Error)]
pub enum AudioEngineError {
    #[error("at least one audio source must be selected")]
    NoSourcesSelected,
    #[error("output sample rate and frame size must be greater than zero")]
    InvalidOutputFormat,
    #[error("silence threshold must be non-negative and warning duration non-zero")]
    InvalidHealthConfiguration,
    #[error("a mixed frame cannot be used as an engine input")]
    MixedFrameAsInput,
    #[error("{audio_source:?} sample rate changed from {previous} Hz to {incoming} Hz")]
    InputSampleRateChanged {
        audio_source: AudioSource,
        previous: u32,
        incoming: u32,
    },
    #[error(
        "source alignment queue capacity {capacity_samples} exceeded by {attempted_samples} samples"
    )]
    SourceAlignmentQueueFull {
        capacity_samples: usize,
        attempted_samples: usize,
    },
    #[error("recorder queue capacity {capacity_samples} exceeded by {attempted_samples} samples")]
    RecorderQueueFull {
        capacity_samples: usize,
        attempted_samples: usize,
    },
    #[error("recorder queue rejected a frame: {0}")]
    RecorderQueueRejected(QueueError),
    #[error(transparent)]
    Frame(#[from] FrameError),
    #[error(transparent)]
    Mixer(#[from] MixerError),
    #[error(transparent)]
    Queue(#[from] QueueError),
    #[error(transparent)]
    Resample(#[from] ResampleError),
}
