//! Platform identity and the boundary for OS-specific adapters.
//!
//! Overlays, focus resolution, text injection, tray integration, hotkeys,
//! autostart, and session monitoring will be implemented per platform. The
//! Phase 1 audio/VAD core intentionally does not depend on any native handle.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Windows,
    Linux,
    Other,
}

impl Platform {
    pub const fn current() -> Self {
        if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "linux") {
            Self::Linux
        } else {
            Self::Other
        }
    }
}
