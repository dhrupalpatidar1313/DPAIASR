//! Phase 1 microphone capture and voice-activity detection pipeline.

pub mod audio;
pub mod core;
pub mod platform;
pub mod vad;

pub use audio::{AudioCapture, AudioCaptureConfig, AudioCaptureError, CaptureEvent};
pub use audio::{AudioPreprocessor, PreprocessingConfig};
pub use core::SessionState;
pub use platform::Platform;
pub use vad::{
    EnergyVad, SileroVad, SileroVadConfig, UtteranceTracker, Vad, VadConfig, VadError, VadEvent,
    VadState,
};
