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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayState {
    Stopped,
    Listening,
    Speaking,
    Transcribing,
    Injecting,
    WaitingForTarget,
    Orphaned,
    Stopping,
    Error,
    Muted,
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

#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "windows")]
pub use windows::target::TargetSnapshot;
#[cfg(target_os = "windows")]
pub use windows::hotkey::HotkeyManager;
#[cfg(target_os = "windows")]
pub use windows::ui::OverlayUI;

#[cfg(target_os = "linux")]
pub use linux::target::TargetSnapshot;
#[cfg(target_os = "linux")]
pub use linux::hotkey::HotkeyManager;
#[cfg(target_os = "linux")]
pub use linux::ui::OverlayUI;

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetSnapshot {}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
impl TargetSnapshot {
    pub fn current() -> Option<Self> {
        Some(Self {})
    }
}
