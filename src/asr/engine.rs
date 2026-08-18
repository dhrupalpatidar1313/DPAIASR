

/// A single recognized segment.
#[derive(Debug, Clone)]
pub struct Segment {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

/// Output of one transcription call.
#[derive(Debug, Clone)]
pub struct Transcript {
    pub text: String,
    pub segments: Vec<Segment>,
}

/// All errors the ASR subsystem can produce.
#[derive(Debug, Clone, thiserror::Error)]
pub enum AsrError {
    #[error("model not loaded: {0}")]
    ModelNotLoaded(String),
    #[error("transcription failed: {0}")]
    Inference(String),
    #[error("empty audio buffer passed to ASR")]
    EmptyBuffer,
    #[error("worker thread panicked")]
    WorkerPanicked,
    #[error("ASR queue is full; utterance dropped")]
    QueueFull,
}

/// Core trait — every concrete backend implements this.
pub trait AsrEngine: Send + 'static {
    /// Transcribe a mono-f32 PCM buffer at 16 000 Hz.
    fn transcribe(&mut self, pcm: &[f32]) -> Result<Transcript, AsrError>;
}

/// Test double — no model file required.
pub struct NullAsr;

impl AsrEngine for NullAsr {
    fn transcribe(&mut self, pcm: &[f32]) -> Result<Transcript, AsrError> {
        if pcm.is_empty() {
            return Err(AsrError::EmptyBuffer);
        }
        Ok(Transcript {
            text: String::new(),
            segments: vec![],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_null_asr_accepts_pcm() {
        let mut asr = NullAsr;
        let pcm = vec![0.0; 16000]; // 1 second of silence
        let result = asr.transcribe(&pcm);
        assert!(result.is_ok());
    }

    #[test]
    fn test_null_asr_rejects_empty_pcm() {
        let mut asr = NullAsr;
        let result = asr.transcribe(&[]);
        assert!(matches!(result, Err(AsrError::EmptyBuffer)));
    }
}
