use std::time::Instant;

#[derive(Debug, Clone, Copy, Eq)]
pub struct TargetSnapshot {
    pub window_id: isize,
    pub process_id: u32,
    pub captured_at: Instant,
}

impl PartialEq for TargetSnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.window_id == other.window_id &&
        self.process_id == other.process_id
    }
}

impl TargetSnapshot {
    pub fn current() -> Option<Self> {
        // On Wayland, we cannot determine the active window ID or PID 
        // due to strict security isolation.
        // We return a dummy snapshot because `wtype` automatically
        // types into the compositor's currently focused window anyway.
        Some(Self {
            window_id: 0,
            process_id: 0,
            captured_at: Instant::now(),
        })
    }

    pub fn is_alive(&self) -> bool {
        // We cannot verify window aliveness on Wayland easily,
        // so we assume the "compositor's focused window" is always alive.
        true
    }
}
