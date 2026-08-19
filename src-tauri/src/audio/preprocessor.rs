#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AudioLevel {
    pub rms: f32,
    pub peak: f32,
    pub is_silent: bool,
}

pub fn measure_level(samples: &[f32], silence_rms_threshold: f32) -> AudioLevel {
    if samples.is_empty() {
        return AudioLevel {
            rms: 0.0,
            peak: 0.0,
            is_silent: true,
        };
    }

    let sum_squares = samples.iter().map(|sample| sample * sample).sum::<f32>();
    let rms = (sum_squares / samples.len() as f32).sqrt();
    let peak = samples
        .iter()
        .map(|sample| sample.abs())
        .fold(0.0_f32, f32::max);

    AudioLevel {
        rms,
        peak,
        is_silent: rms <= silence_rms_threshold,
    }
}

pub fn apply_hard_limiter(samples: &mut [f32], threshold: f32) {
    for sample in samples {
        *sample = sample.clamp(-threshold, threshold);
    }
}
