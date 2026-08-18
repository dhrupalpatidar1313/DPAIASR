use std::time::Instant;
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId, IsWindow};
use windows::Win32::Foundation::HWND;

/// Represents a captured target window for text injection.
#[derive(Debug, Clone, Copy, Eq)]
pub struct TargetSnapshot {
    pub hwnd: isize,
    pub process_id: u32,
    pub thread_id: u32,
    pub captured_at: Instant,
}

impl PartialEq for TargetSnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.hwnd == other.hwnd &&
        self.process_id == other.process_id &&
        self.thread_id == other.thread_id
    }
}

impl TargetSnapshot {
    /// Captures the currently focused (active) window.
    pub fn current() -> Option<Self> {
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.0.is_null() {
            return None;
        }
        
        let mut process_id = 0;
        let thread_id = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process_id)) };
        
        Some(Self {
            hwnd: hwnd.0 as isize,
            process_id,
            thread_id,
            captured_at: Instant::now(),
        })
    }

    pub fn is_alive(&self) -> bool {
        unsafe { IsWindow(Some(HWND(self.hwnd as *mut _))).into() }
    }
}
