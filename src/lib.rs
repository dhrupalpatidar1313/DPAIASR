//! Phase 1 microphone capture and voice-activity detection pipeline.

pub mod asr;
pub mod audio;
pub mod core;
pub mod injection;
pub mod pipeline;
pub mod platform;
pub mod session;
pub mod vad;

pub use asr::{AsrEngine, AsrError, NullAsr, Segment, Transcript, CpuWhisperAsr};
#[cfg(feature = "cuda")]
pub use asr::GpuWhisperAsr;
pub use asr::{AsrWorker, AsrWorkerConfig, AsrWorkerHandle};
pub use asr::{BackendResolution, BackendResolver, BackendResolverConfig, BackendPreference};
pub use asr::{ModelManager, ModelManagerConfig, ModelManagerError};
pub use audio::{AudioCapture, AudioCaptureConfig, AudioCaptureError, CaptureEvent};
pub use audio::{AudioPreprocessor, PreprocessingConfig};
pub use core::SessionState;
pub use injection::{InjectionResult, InjectionWorker, InjectionWorkerConfig, InjectionWorkerHandle};
pub use pipeline::{AsrJob, InjectionJob};
pub use platform::Platform;
pub use vad::{
    EnergyVad, SileroVad, SileroVadConfig, UtteranceTracker, Vad, VadConfig, VadError, VadEvent,
    VadState,
};
