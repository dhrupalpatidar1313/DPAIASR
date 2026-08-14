//! Platform-independent voice activity detection.

mod energy;
mod silero;
mod tracker;
mod r#trait;

pub use energy::EnergyVad;
pub use silero::{SileroVad, SileroVadConfig};
pub use tracker::UtteranceTracker;
pub use r#trait::{Vad, VadConfig, VadError, VadEvent, VadState};
