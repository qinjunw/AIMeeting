use std::time::Duration;

use thiserror::Error;

use super::frame::{AudioFrame, AudioSource};

#[derive(Debug)]
pub struct SyntheticAudioSource {
    source: AudioSource,
    sample_rate: u32,
    channels: u16,
    frame_duration: Duration,
    frame_count: u64,
    next_frame_index: u64,
    generated_samples: u64,
}

impl SyntheticAudioSource {
    pub fn silence(
        source: AudioSource,
        sample_rate: u32,
        channels: u16,
        frame_duration: Duration,
        total_duration: Duration,
    ) -> Result<Self, SyntheticSourceError> {
        if sample_rate == 0 || channels == 0 || frame_duration.is_zero() {
            return Err(SyntheticSourceError::InvalidConfiguration);
        }
        if !total_duration
            .as_nanos()
            .is_multiple_of(frame_duration.as_nanos())
        {
            return Err(SyntheticSourceError::NonIntegralFrameCount);
        }

        Ok(Self {
            source,
            sample_rate,
            channels,
            frame_duration,
            frame_count: (total_duration.as_nanos() / frame_duration.as_nanos()) as u64,
            next_frame_index: 0,
            generated_samples: 0,
        })
    }

    pub fn next_frame(&mut self) -> Option<AudioFrame> {
        if self.next_frame_index >= self.frame_count {
            return None;
        }

        let frame_end_nanos = (self.next_frame_index as u128 + 1) * self.frame_duration.as_nanos();
        let cumulative_samples =
            frame_end_nanos * self.sample_rate as u128 / Duration::from_secs(1).as_nanos();
        let frame_samples = cumulative_samples as u64 - self.generated_samples;
        let timestamp = self.frame_duration.mul_f64(self.next_frame_index as f64);
        let samples = vec![0.0; frame_samples as usize * self.channels as usize];

        self.generated_samples = cumulative_samples as u64;
        self.next_frame_index += 1;

        AudioFrame::new(
            self.source,
            timestamp,
            self.sample_rate,
            self.channels,
            samples,
        )
        .ok()
    }

    pub fn generated_samples(&self) -> u64 {
        self.generated_samples
    }
}

#[derive(Debug, Error)]
pub enum SyntheticSourceError {
    #[error("sample rate, channels, and frame duration must be non-zero")]
    InvalidConfiguration,
    #[error("total duration must contain an integral number of frames")]
    NonIntegralFrameCount,
}
