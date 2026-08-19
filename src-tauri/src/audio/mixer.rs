use std::collections::VecDeque;
use std::time::Duration;

use thiserror::Error;

use super::frame::{AudioFrame, AudioSource, FrameError};
use super::preprocessor::apply_hard_limiter;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MixerConfig {
    pub microphone_gain: f32,
    pub system_gain: f32,
    pub limiter_threshold: f32,
}

#[derive(Debug)]
pub struct AudioMixer {
    config: MixerConfig,
    last_timestamp: Option<Duration>,
}

impl AudioMixer {
    pub fn new(config: MixerConfig) -> Result<Self, MixerError> {
        if !config.microphone_gain.is_finite()
            || !config.system_gain.is_finite()
            || config.microphone_gain < 0.0
            || config.system_gain < 0.0
        {
            return Err(MixerError::InvalidGain);
        }
        if !config.limiter_threshold.is_finite()
            || !(0.0..=1.0).contains(&config.limiter_threshold)
            || config.limiter_threshold == 0.0
        {
            return Err(MixerError::InvalidLimiterThreshold);
        }

        Ok(Self {
            config,
            last_timestamp: None,
        })
    }

    pub fn mix(
        &mut self,
        microphone: Option<&AudioFrame>,
        system: Option<&AudioFrame>,
    ) -> Result<AudioFrame, MixerError> {
        let microphone = microphone.map(AudioFrame::to_mono).transpose()?;
        let system = system.map(AudioFrame::to_mono).transpose()?;
        let reference = microphone
            .as_ref()
            .or(system.as_ref())
            .ok_or(MixerError::NoSources)?;

        if let (Some(microphone), Some(system)) = (&microphone, &system) {
            if microphone.sample_rate() != system.sample_rate()
                || microphone.timestamp() != system.timestamp()
                || microphone.sample_frames() != system.sample_frames()
            {
                return Err(MixerError::SourcesNotAligned);
            }
        }

        if self
            .last_timestamp
            .is_some_and(|last| reference.timestamp() < last)
        {
            return Err(MixerError::TimestampRegression);
        }

        let mut samples = Vec::with_capacity(reference.sample_frames());
        for index in 0..reference.sample_frames() {
            let microphone_sample = microphone.as_ref().map_or(0.0, |frame| {
                frame.samples()[index] * self.config.microphone_gain
            });
            let system_sample = system.as_ref().map_or(0.0, |frame| {
                frame.samples()[index] * self.config.system_gain
            });
            samples.push(microphone_sample + system_sample);
        }
        apply_hard_limiter(&mut samples, self.config.limiter_threshold);
        self.last_timestamp = Some(reference.timestamp());

        AudioFrame::new(
            AudioSource::Mixed,
            reference.timestamp(),
            reference.sample_rate(),
            1,
            samples,
        )
        .map_err(MixerError::from)
    }
}

#[derive(Debug)]
pub struct BoundedFrameQueue {
    capacity_samples: usize,
    buffered_samples: usize,
    high_watermark_samples: usize,
    last_timestamp: Option<Duration>,
    frames: VecDeque<AudioFrame>,
}

impl BoundedFrameQueue {
    pub fn new(capacity_samples: usize) -> Result<Self, QueueError> {
        if capacity_samples == 0 {
            return Err(QueueError::ZeroCapacity);
        }
        Ok(Self {
            capacity_samples,
            buffered_samples: 0,
            high_watermark_samples: 0,
            last_timestamp: None,
            frames: VecDeque::new(),
        })
    }

    pub fn push(&mut self, frame: AudioFrame) -> Result<(), QueueError> {
        if let Some(last_timestamp) = self.last_timestamp {
            if frame.timestamp() < last_timestamp {
                return Err(QueueError::TimestampRegression {
                    previous: last_timestamp,
                    incoming: frame.timestamp(),
                });
            }
        }

        let attempted_samples = self.buffered_samples + frame.samples().len();
        if attempted_samples > self.capacity_samples {
            return Err(QueueError::CapacityExceeded {
                capacity_samples: self.capacity_samples,
                attempted_samples,
            });
        }

        self.buffered_samples = attempted_samples;
        self.high_watermark_samples = self.high_watermark_samples.max(self.buffered_samples);
        self.last_timestamp = Some(frame.timestamp());
        self.frames.push_back(frame);
        Ok(())
    }

    pub fn pop(&mut self) -> Option<AudioFrame> {
        let frame = self.frames.pop_front()?;
        self.buffered_samples -= frame.samples().len();
        Some(frame)
    }

    pub fn buffered_samples(&self) -> usize {
        self.buffered_samples
    }

    pub fn high_watermark_samples(&self) -> usize {
        self.high_watermark_samples
    }
}

#[derive(Debug, Error)]
pub enum MixerError {
    #[error("at least one audio source is required")]
    NoSources,
    #[error("source gains must be finite and non-negative")]
    InvalidGain,
    #[error("limiter threshold must be in the range (0, 1]")]
    InvalidLimiterThreshold,
    #[error("audio sources must have matching timestamps, sample rates, and lengths")]
    SourcesNotAligned,
    #[error("mixed frame timestamp moved backwards")]
    TimestampRegression,
    #[error(transparent)]
    Frame(#[from] FrameError),
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum QueueError {
    #[error("audio queue capacity must be greater than zero")]
    ZeroCapacity,
    #[error("incoming timestamp {incoming:?} is older than {previous:?}")]
    TimestampRegression {
        previous: Duration,
        incoming: Duration,
    },
    #[error("queue capacity {capacity_samples} would be exceeded by {attempted_samples} samples")]
    CapacityExceeded {
        capacity_samples: usize,
        attempted_samples: usize,
    },
}
