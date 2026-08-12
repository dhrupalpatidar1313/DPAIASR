//! Phase 1 lifecycle types.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Stopped,
    Running,
    Stopping,
    Failed,
}
