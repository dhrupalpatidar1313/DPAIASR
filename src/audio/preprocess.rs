//! Audio normalization, downmixing, resampling, and frame assembly.

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreprocessingConfig {
    pub target_sample_rate: u32,
    pub frame_duration_ms: u32,
}

impl Default for PreprocessingConfig {
    fn default() -> Self {
        Self {
            target_sample_rate: 16_000,
            frame_duration_ms: 30,
        }
    }
}

impl PreprocessingConfig {
    pub fn frame_size(self) -> Result<usize, PreprocessingError> {
        let samples = u64::from(self.target_sample_rate)
            .checked_mul(u64::from(self.frame_duration_ms))
            .and_then(|value| value.checked_div(1_000))
            .ok_or(PreprocessingError::InvalidFrameConfiguration)?;
        usize::try_from(samples).map_err(|_| PreprocessingError::InvalidFrameConfiguration)
    }
}

#[derive(Debug, Error, PartialEq)]
pub enum PreprocessingError {
    #[error("sample rate must be greater than zero")]
    InvalidSampleRate,
    #[error("channel count must be greater than zero")]
    InvalidChannelCount,
    #[error("frame configuration does not produce a valid frame size")]
    InvalidFrameConfiguration,
}

#[derive(Debug)]
pub struct AudioPreprocessor {
    config: PreprocessingConfig,
    source_sample_rate: u32,
    channels: usize,
    pending: Vec<f32>,
    resample_remainder: Vec<f32>,
}

impl AudioPreprocessor {
    pub fn new(
        source_sample_rate: u32,
        channels: usize,
        config: PreprocessingConfig,
    ) -> Result<Self, PreprocessingError> {
        if source_sample_rate == 0 {
            return Err(PreprocessingError::InvalidSampleRate);
        }
        if channels == 0 {
            return Err(PreprocessingError::InvalidChannelCount);
        }
        config.frame_size()?;

        Ok(Self {
            config,
            source_sample_rate,
            channels,
            pending: Vec::new(),
            resample_remainder: Vec::new(),
        })
    }

    pub fn push_interleaved(&mut self, samples: &[f32]) -> Vec<Vec<f32>> {
        let mono = downmix_to_mono(samples, self.channels);
        let resampled = resample_linear(
            &mono,
            self.source_sample_rate,
            self.config.target_sample_rate,
            &mut self.resample_remainder,
        );
        self.pending.extend(resampled);

        let frame_size = self.config.frame_size().expect("validated in constructor");
        let mut frames = Vec::new();
        while self.pending.len() >= frame_size {
            frames.push(self.pending.drain(..frame_size).collect());
        }
        frames
    }
}

pub fn downmix_to_mono(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels == 1 {
        return interleaved.to_vec();
    }

    interleaved
        .chunks(channels)
        .map(|frame| frame.iter().copied().sum::<f32>() / frame.len() as f32)
        .collect()
}

fn resample_linear(
    input: &[f32],
    source_rate: u32,
    target_rate: u32,
    remainder: &mut Vec<f32>,
) -> Vec<f32> {
    if source_rate == target_rate {
        return input.to_vec();
    }

    remainder.extend_from_slice(input);
    let ratio = source_rate as f64 / target_rate as f64;
    let output_len = ((remainder.len().saturating_sub(1)) as f64 / ratio).floor() as usize;
    let mut output = Vec::with_capacity(output_len);

    for index in 0..output_len {
        let position = index as f64 * ratio;
        let left = position.floor() as usize;
        let fraction = (position - left as f64) as f32;
        let right = (left + 1).min(remainder.len() - 1);
        output.push(remainder[left] * (1.0 - fraction) + remainder[right] * fraction);
    }

    let consumed = (output_len as f64 * ratio).floor() as usize;
    remainder.drain(..consumed.min(remainder.len()));
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downmix_averages_channels() {
        assert_eq!(downmix_to_mono(&[1.0, -1.0, 0.5, 0.5], 2), vec![0.0, 0.5]);
    }

    #[test]
    fn frame_assembler_emits_480_sample_frames() {
        let mut preprocessor = AudioPreprocessor::new(16_000, 1, PreprocessingConfig::default())
            .expect("valid config");
        let frames = preprocessor.push_interleaved(&vec![0.1; 960]);
        assert_eq!(frames.len(), 2);
        assert!(frames.iter().all(|frame| frame.len() == 480));
    }
}
