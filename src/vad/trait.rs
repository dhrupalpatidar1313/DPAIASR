//! Common VAD interface, configuration, states, and events.

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VadState {
    Silence,
    Speaking,
}

#[derive(Debug, Clone, PartialEq)]
pub enum VadEvent {
    StateChanged(VadState),
    AudioLevel {
        rms: f32,
        dbfs: f32,
    },
    SpeechProbability {
        probability: f32,
    },
    Utterance {
        id: u64,
        audio: Vec<f32>,
        duration_frames: usize,
    },
}

#[derive(Debug, Error, PartialEq)]
pub enum VadError {
    #[error("VAD frame must contain at least one sample")]
    EmptyFrame,
    #[error("VAD frame size {actual} does not match expected size {expected}")]
    InvalidFrameSize { actual: usize, expected: usize },
    #[error("VAD sample rate and frame duration do not produce an integer frame size")]
    InvalidFrameConfiguration,
    #[error("unsupported sample rate for VAD: {0} Hz")]
    UnsupportedSampleRate(u32),
    #[error("model file not found at '{0}'")]
    ModelNotFound(std::path::PathBuf),
    #[error("failed to initialize ONNX session: {0}")]
    SessionInit(String),
    #[error("ONNX inference failed: {0}")]
    Inference(String),
}

pub trait Vad {
    fn process_frame(&mut self, frame: &[f32]) -> Result<Vec<VadEvent>, VadError>;
    fn reset(&mut self);
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VadConfig {
    pub sample_rate: u32,
    pub frame_duration_ms: u32,
    pub start_threshold_dbfs: f32,
    pub end_threshold_dbfs: f32,
    pub start_confirmation_frames: usize,
    pub hangover_frames: usize,
    pub minimum_utterance_frames: usize,
    pub pre_roll_frames: usize,
    pub maximum_utterance_frames: usize,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            sample_rate: 16_000,
            frame_duration_ms: 30,
            start_threshold_dbfs: -45.0,
            end_threshold_dbfs: -50.0,
            start_confirmation_frames: 3,
            hangover_frames: 10,
            minimum_utterance_frames: 4,
            pre_roll_frames: 5,
            maximum_utterance_frames: 500,
        }
    }
}

impl VadConfig {
    pub fn frame_size(self) -> Result<usize, VadError> {
        let samples = u64::from(self.sample_rate)
            .checked_mul(u64::from(self.frame_duration_ms))
            .and_then(|value| value.checked_div(1_000))
            .ok_or(VadError::InvalidFrameConfiguration)?;
        usize::try_from(samples).map_err(|_| VadError::InvalidFrameConfiguration)
    }
}
