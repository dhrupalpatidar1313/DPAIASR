//! Platform-independent microphone capture and audio preparation.

mod capture;
mod preprocess;

pub use capture::{AudioCapture, AudioCaptureConfig, AudioCaptureError, CaptureEvent};
pub use preprocess::{AudioPreprocessor, PreprocessingConfig, PreprocessingError};
