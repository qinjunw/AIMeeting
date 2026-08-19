use std::time::Duration;

use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AudioSource {
    Microphone,
    System,
    Mixed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AudioFrame {
    source: AudioSource,
    timestamp: Duration,
    sample_rate: u32,
    channels: u16,
    samples: Vec<f32>,
}

impl AudioFrame {
    pub fn new(
        source: AudioSource,
        timestamp: Duration,
        sample_rate: u32,
        channels: u16,
        samples: Vec<f32>,
    ) -> Result<Self, FrameError> {
        if sample_rate == 0 {
            return Err(FrameError::InvalidSampleRate);
        }
        if channels == 0 {
            return Err(FrameError::InvalidChannelCount);
        }
        if !samples.len().is_multiple_of(channels as usize) {
            return Err(FrameError::IncompleteInterleavedFrame {
                samples: samples.len(),
                channels,
            });
        }

        Ok(Self {
            source,
            timestamp,
            sample_rate,
            channels,
            samples,
        })
    }

    pub fn source(&self) -> AudioSource {
        self.source
    }

    pub fn timestamp(&self) -> Duration {
        self.timestamp
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    pub fn sample_frames(&self) -> usize {
        self.samples.len() / self.channels as usize
    }

    pub fn to_mono(&self) -> Result<Self, FrameError> {
        if self.channels == 1 {
            return Ok(self.clone());
        }

        let channel_count = self.channels as usize;
        let samples = self
            .samples
            .chunks_exact(channel_count)
            .map(|sample_frame| sample_frame.iter().sum::<f32>() / channel_count as f32)
            .collect();

        Self::new(self.source, self.timestamp, self.sample_rate, 1, samples)
    }
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum FrameError {
    #[error("sample rate must be greater than zero")]
    InvalidSampleRate,
    #[error("channel count must be greater than zero")]
    InvalidChannelCount,
    #[error("{samples} samples do not contain complete frames for {channels} channels")]
    IncompleteInterleavedFrame { samples: usize, channels: u16 },
}
