use crate::session::{SessionId, UtteranceId};
use crate::platform::TargetSnapshot;
use crate::asr::Transcript;

#[derive(Debug, Clone)]
pub struct AsrJob {
    pub session_id: SessionId,
    pub utterance_id: UtteranceId,
    pub audio: Vec<f32>,
    pub target: Option<TargetSnapshot>,
}

#[derive(Debug, Clone)]
pub struct InjectionJob {
    pub session_id: SessionId,
    pub utterance_id: UtteranceId,
    pub transcript: Transcript,
    pub target: Option<TargetSnapshot>,
}
