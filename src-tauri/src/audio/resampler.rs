use thiserror::Error;

pub fn resample_linear(
    input: &[f32],
    input_rate: u32,
    output_rate: u32,
) -> Result<Vec<f32>, ResampleError> {
    let mut resampler = StreamingLinearResampler::new(input_rate, output_rate)?;
    Ok(resampler.process(input))
}

#[derive(Debug)]
pub struct StreamingLinearResampler {
    input_rate: u64,
    output_rate: u64,
    previous_sample: Option<f32>,
    previous_input_index: u64,
    next_output_index: u64,
}

impl StreamingLinearResampler {
    pub fn new(input_rate: u32, output_rate: u32) -> Result<Self, ResampleError> {
        if input_rate == 0 || output_rate == 0 {
            return Err(ResampleError::InvalidSampleRate);
        }

        Ok(Self {
            input_rate: input_rate as u64,
            output_rate: output_rate as u64,
            previous_sample: None,
            previous_input_index: 0,
            next_output_index: 0,
        })
    }

    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        let mut output = Vec::with_capacity(
            input.len() * self.output_rate as usize / self.input_rate as usize + 2,
        );

        for &current_sample in input {
            let Some(previous_sample) = self.previous_sample else {
                self.previous_sample = Some(current_sample);
                output.push(current_sample);
                self.next_output_index = 1;
                continue;
            };

            let current_input_index = self.previous_input_index + 1;
            while (self.next_output_index as u128) * (self.input_rate as u128)
                <= (current_input_index as u128) * (self.output_rate as u128)
            {
                let output_position = (self.next_output_index as u128) * (self.input_rate as u128);
                let segment_start =
                    (self.previous_input_index as u128) * (self.output_rate as u128);
                let fraction = (output_position - segment_start) as f32 / self.output_rate as f32;
                output.push(previous_sample + (current_sample - previous_sample) * fraction);
                self.next_output_index += 1;
            }

            self.previous_sample = Some(current_sample);
            self.previous_input_index = current_input_index;
        }

        output
    }

    pub fn buffered_samples(&self) -> usize {
        usize::from(self.previous_sample.is_some())
    }
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum ResampleError {
    #[error("input and output sample rates must be greater than zero")]
    InvalidSampleRate,
}
