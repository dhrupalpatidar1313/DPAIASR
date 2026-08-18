#![cfg(feature = "cuda")]

use crate::asr::engine::{AsrEngine, AsrError, Segment, Transcript};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState};
use std::path::Path;

pub struct GpuWhisperAsr {
    _ctx: WhisperContext, // Must outlive state
    state: WhisperState,
    n_threads: i32,
}

impl GpuWhisperAsr {
    pub fn new(model_path: &Path, gpu_device: u32, n_threads: u32) -> Result<Self, AsrError> {
        // Install global logging hook for whisper-rs exactly once.
        static LOGGING_INIT: std::sync::Once = std::sync::Once::new();
        LOGGING_INIT.call_once(|| {
            whisper_rs::install_logging_hooks();
        });

        let mut ctx_params = WhisperContextParameters::default();
        ctx_params.use_gpu = true;
        ctx_params.gpu_device = gpu_device as i32;

        let model_path_str = model_path
            .to_str()
            .ok_or_else(|| AsrError::ModelNotLoaded("model path is not valid UTF-8".into()))?;

        let ctx = WhisperContext::new_with_params(model_path_str, ctx_params)
            .map_err(|e| AsrError::ModelNotLoaded(e.to_string()))?;

        let state = ctx
            .create_state()
            .map_err(|e| AsrError::ModelNotLoaded(e.to_string()))?;

        Ok(Self {
            _ctx: ctx,
            state,
            n_threads: n_threads as i32,
        })
    }
}

impl AsrEngine for GpuWhisperAsr {
    fn transcribe(&mut self, pcm: &[f32]) -> Result<Transcript, AsrError> {
        if pcm.is_empty() {
            return Err(AsrError::EmptyBuffer);
        }

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_n_threads(self.n_threads);
        params.set_language(Some("en"));
        params.set_translate(false);
        params.set_no_context(true);
        params.set_single_segment(false);
        params.set_suppress_nst(false);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);

        self.state
            .full(params, pcm)
            .map_err(|e| AsrError::Inference(e.to_string()))?;

        let mut segments = Vec::new();
        let mut full_text = String::new();

        for seg in self.state.as_iter() {
            let text = seg.to_str_lossy().unwrap_or_default().into_owned();
            full_text.push_str(&text);
            segments.push(Segment {
                start_ms: seg.start_timestamp() * 10,
                end_ms: seg.end_timestamp() * 10,
                text,
            });
        }

        Ok(Transcript {
            text: full_text.trim().to_owned(),
            segments,
        })
    }
}
