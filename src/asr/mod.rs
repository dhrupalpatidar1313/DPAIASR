pub mod backend;
pub mod cpu;
pub mod engine;
#[cfg(feature = "cuda")]
pub mod gpu;
pub mod model_manager;
pub mod resolver;
pub mod worker;

pub use backend::{AsrBackendKind, BackendPreference};
pub use cpu::CpuWhisperAsr;
pub use engine::{AsrEngine, AsrError, NullAsr, Segment, Transcript};
pub use model_manager::{ModelManager, ModelManagerConfig, ModelManagerError};
pub use resolver::{BackendResolution, BackendResolver, BackendResolverConfig};
pub use worker::{AsrWorker, AsrWorkerConfig, AsrWorkerHandle};

#[cfg(feature = "cuda")]
pub use gpu::GpuWhisperAsr;
