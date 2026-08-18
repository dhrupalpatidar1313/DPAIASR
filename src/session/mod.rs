/// Unique identifier for a dictation session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(pub u64);

/// Unique identifier for an utterance within a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UtteranceId(pub u64);

/// Session lifecycle state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Stopped,
    Listening,
    Stopping,
    Muted,
}

/// Voice activity state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeechState {
    Silent,
    Speaking,
}

/// Pipeline processing state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessingState {
    Idle,
    Transcribing,
    Injecting,
    WaitingForTarget,
    Orphaned,
    Error,
}

pub struct SessionController {
    pub session: SessionState,
    pub speech: SpeechState,
    pub processing: ProcessingState,
}

impl SessionController {
    pub fn new() -> Self {
        Self {
            session: SessionState::Stopped,
            speech: SpeechState::Silent,
            processing: ProcessingState::Idle,
        }
    }

    pub fn toggle(&mut self) -> SessionState {
        self.session = match self.session {
            SessionState::Stopped => SessionState::Listening,
            SessionState::Listening => SessionState::Stopped,
            _ => SessionState::Stopped,
        };
        self.session
    }

    pub fn set_session(&mut self, state: SessionState) {
        self.session = state;
    }

    pub fn set_speech(&mut self, state: SpeechState) {
        self.speech = state;
    }

    pub fn set_processing(&mut self, state: ProcessingState) {
        self.processing = state;
    }
}



