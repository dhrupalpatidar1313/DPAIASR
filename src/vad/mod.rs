//! Platform-independent voice activity detection.

mod energy;
mod r#trait;

pub use energy::EnergyVad;
pub use r#trait::{Vad, VadConfig, VadError, VadEvent, VadState};
