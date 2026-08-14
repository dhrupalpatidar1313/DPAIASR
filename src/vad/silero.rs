//! Neural network voice activity detection powered by Silero VAD (ONNX Runtime).

use std::path::{Path, PathBuf};

use ort::session::Session;
use ort::value::Value;

use super::tracker::UtteranceTracker;
use super::{Vad, VadConfig, VadError, VadEvent, VadState};

const DEFAULT_MODEL_PATH: &str = "assets/models/silero_vad.onnx";
const FALLBACK_MODEL_PATH: &str = "assets/silero_vad.onnx";
const ENV_MODEL_PATH: &str = "SILERO_VAD_MODEL_PATH";

/// Hidden state dimension for Silero VAD v5: shape is `[2, 1, 128]` (256 floats).
const STATE_SIZE: usize = 256;

/// Configuration options for the [`SileroVad`] detector.
#[derive(Debug, Clone, PartialEq)]
pub struct SileroVadConfig {
    /// Optional explicit path to the `silero_vad.onnx` model file.
    /// If `None`, standard search locations and the `SILERO_VAD_MODEL_PATH` env var are used.
    pub model_path: Option<PathBuf>,
    /// Sample rate in Hertz (must be 8000 or 16000).
    pub sample_rate: u32,
    /// Probability threshold (between 0.0 and 1.0) above which speech is detected.
    pub start_threshold: f32,
    /// Probability threshold (between 0.0 and 1.0) below which silence is detected.
    pub end_threshold: f32,
    /// Number of consecutive speech frames required to confirm speech start.
    pub start_confirmation_frames: usize,
    /// Number of consecutive silent frames required to trigger utterance completion.
    pub hangover_frames: usize,
    /// Minimum utterance length in frames to be emitted.
    pub minimum_utterance_frames: usize,
    /// Number of pre-speech audio frames to prepend to confirmed utterances.
    pub pre_roll_frames: usize,
    /// Maximum allowed utterance length in frames before forced segmentation.
    pub maximum_utterance_frames: usize,
}

impl Default for SileroVadConfig {
    fn default() -> Self {
        Self {
            model_path: None,
            sample_rate: 16_000,
            start_threshold: 0.5,
            end_threshold: 0.35,
            start_confirmation_frames: 2,
            hangover_frames: 10,
            minimum_utterance_frames: 4,
            pre_roll_frames: 5,
            maximum_utterance_frames: 500,
        }
    }
}

impl SileroVadConfig {
    /// Returns the required frame size (in samples) for the configured sample rate.
    ///
    /// Silero VAD requires:
    /// - 512 samples for 16,000 Hz (32 ms)
    /// - 256 samples for 8,000 Hz (32 ms)
    pub fn frame_size(&self) -> Result<usize, VadError> {
        match self.sample_rate {
            16_000 => Ok(512),
            8_000 => Ok(256),
            other => Err(VadError::UnsupportedSampleRate(other)),
        }
    }

    /// Converts this config into a standard [`VadConfig`] for the [`UtteranceTracker`].
    pub fn to_vad_config(&self) -> Result<VadConfig, VadError> {
        // At 16kHz with 512 samples or 8kHz with 256 samples, frame duration is 32ms.
        let frame_duration_ms = match self.sample_rate {
            16_000 | 8_000 => 32,
            other => return Err(VadError::UnsupportedSampleRate(other)),
        };

        Ok(VadConfig {
            sample_rate: self.sample_rate,
            frame_duration_ms,
            start_threshold_dbfs: -45.0,
            end_threshold_dbfs: -50.0,
            start_confirmation_frames: self.start_confirmation_frames,
            hangover_frames: self.hangover_frames,
            minimum_utterance_frames: self.minimum_utterance_frames,
            pre_roll_frames: self.pre_roll_frames,
            maximum_utterance_frames: self.maximum_utterance_frames,
        })
    }
}

/// Neural Voice Activity Detector utilizing the Silero VAD ONNX model.
pub struct SileroVad {
    config: SileroVadConfig,
    session: Session,
    state: Vec<f32>,
    tracker: UtteranceTracker,
    sample_rate_i64: i64,
}

impl std::fmt::Debug for SileroVad {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SileroVad")
            .field("config", &self.config)
            .field("state_len", &self.state.len())
            .field("tracker", &self.tracker)
            .finish_non_exhaustive()
    }
}

impl SileroVad {
    /// Creates a new [`SileroVad`] detector, locating the model file automatically
    /// if not specified in `config`.
    ///
    /// # Search Order for Model:
    /// 1. `config.model_path` (if set)
    /// 2. `SILERO_VAD_MODEL_PATH` environment variable
    /// 3. `assets/models/silero_vad.onnx`
    /// 4. `assets/silero_vad.onnx`
    ///
    /// # Errors
    ///
    /// Returns [`VadError::ModelNotFound`] if no model file could be located,
    /// or [`VadError::SessionInit`] if ONNX runtime session creation fails.
    pub fn new(config: SileroVadConfig) -> Result<Self, VadError> {
        let model_path = resolve_model_path(config.model_path.as_deref())?;
        Self::from_file(model_path, config)
    }

    /// Creates a new [`SileroVad`] detector by loading the ONNX model from the specified file path.
    ///
    /// # Errors
    ///
    /// Returns [`VadError::ModelNotFound`] if the path does not exist,
    /// or [`VadError::SessionInit`] if ONNX Runtime fails to initialize.
    pub fn from_file(path: impl AsRef<Path>, config: SileroVadConfig) -> Result<Self, VadError> {
        let path = path.as_ref();
        if !path.exists() {
            return Err(VadError::ModelNotFound(path.to_path_buf()));
        }

        let session = Session::builder()
            .map_err(|e| VadError::SessionInit(e.to_string()))?
            .commit_from_file(path)
            .map_err(|e| VadError::SessionInit(e.to_string()))?;

        Self::init_with_session(session, config)
    }

    /// Creates a new [`SileroVad`] detector from in-memory ONNX model bytes.
    ///
    /// # Errors
    ///
    /// Returns [`VadError::SessionInit`] if ONNX Runtime fails to load the model bytes.
    pub fn from_memory(model_bytes: &[u8], config: SileroVadConfig) -> Result<Self, VadError> {
        let session = Session::builder()
            .map_err(|e| VadError::SessionInit(e.to_string()))?
            .commit_from_memory(model_bytes)
            .map_err(|e| VadError::SessionInit(e.to_string()))?;

        Self::init_with_session(session, config)
    }

    fn init_with_session(session: Session, config: SileroVadConfig) -> Result<Self, VadError> {
        let vad_config = config.to_vad_config()?;
        let tracker = UtteranceTracker::new(vad_config)?;
        let sample_rate_i64 = i64::from(config.sample_rate);

        Ok(Self {
            config,
            session,
            state: vec![0.0_f32; STATE_SIZE],
            tracker,
            sample_rate_i64,
        })
    }

    /// Returns a reference to the active configuration.
    #[inline]
    pub fn config(&self) -> &SileroVadConfig {
        &self.config
    }

    /// Returns the current VAD state (`Silence` or `Speaking`).
    #[inline]
    pub fn state(&self) -> VadState {
        self.tracker.state()
    }
}

impl Vad for SileroVad {
    fn process_frame(&mut self, frame: &[f32]) -> Result<Vec<VadEvent>, VadError> {
        if frame.is_empty() {
            return Err(VadError::EmptyFrame);
        }

        let expected_frame_size = self.config.frame_size()?;
        if frame.len() != expected_frame_size {
            return Err(VadError::InvalidFrameSize {
                actual: frame.len(),
                expected: expected_frame_size,
            });
        }

        // Calculate RMS and dBFS for observability/diagnostics
        let rms =
            (frame.iter().map(|sample| sample * sample).sum::<f32>() / frame.len() as f32).sqrt();
        let dbfs = 20.0 * rms.max(f32::EPSILON).log10();

        // Build ONNX input tensors
        let input_value = Value::from_array(([1, frame.len()], frame.to_vec()))
            .map_err(|e| VadError::Inference(e.to_string()))?;
        let state_value = Value::from_array(([2, 1, 128], self.state.clone()))
            .map_err(|e| VadError::Inference(e.to_string()))?;
        let sr_value = Value::from_array(((), vec![self.sample_rate_i64]))
            .map_err(|e| VadError::Inference(e.to_string()))?;

        let inputs = ort::inputs![
            "input" => input_value,
            "state" => state_value,
            "sr" => sr_value,
        ];

        // Execute inference
        let outputs = self
            .session
            .run(inputs)
            .map_err(|e| VadError::Inference(e.to_string()))?;

        // Extract speech probability
        let (_output_shape, output_slice) = outputs["output"]
            .try_extract_tensor::<f32>()
            .map_err(|e| VadError::Inference(e.to_string()))?;
        let probability = output_slice.first().copied().unwrap_or(0.0);

        // Update LSTM state tensor
        let (_state_shape, next_state_slice) = outputs["stateN"]
            .try_extract_tensor::<f32>()
            .map_err(|e| VadError::Inference(e.to_string()))?;
        if next_state_slice.len() == self.state.len() {
            self.state.copy_from_slice(next_state_slice);
        }

        // Apply hysteresis thresholding
        let is_speech = match self.tracker.state() {
            VadState::Silence => probability >= self.config.start_threshold,
            VadState::Speaking => probability >= self.config.end_threshold,
        };

        let mut events = vec![
            VadEvent::AudioLevel { rms, dbfs },
            VadEvent::SpeechProbability { probability },
        ];
        self.tracker.process_frame(frame, is_speech, &mut events);
        Ok(events)
    }

    fn reset(&mut self) {
        self.state.fill(0.0);
        self.tracker.reset();
    }
}

fn resolve_model_path(explicit: Option<&Path>) -> Result<PathBuf, VadError> {
    if let Some(path) = explicit {
        if path.exists() {
            return Ok(path.to_path_buf());
        }
        return Err(VadError::ModelNotFound(path.to_path_buf()));
    }

    if let Ok(env_path) = std::env::var(ENV_MODEL_PATH) {
        let path = PathBuf::from(env_path);
        if path.exists() {
            return Ok(path);
        }
    }

    let default_path = PathBuf::from(DEFAULT_MODEL_PATH);
    if default_path.exists() {
        return Ok(default_path);
    }

    let fallback_path = PathBuf::from(FALLBACK_MODEL_PATH);
    if fallback_path.exists() {
        return Ok(fallback_path);
    }

    // Try relative to executable directory
    if let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
    {
        let candidate = exe_dir.join(DEFAULT_MODEL_PATH);
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    Err(VadError::ModelNotFound(default_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silero_vad_initializes_and_processes_frames() {
        let config = SileroVadConfig {
            start_confirmation_frames: 2,
            hangover_frames: 2,
            minimum_utterance_frames: 2,
            pre_roll_frames: 1,
            ..SileroVadConfig::default()
        };

        let frame_size = config.frame_size().expect("frame size");
        assert_eq!(frame_size, 512);

        let mut vad = SileroVad::new(config).expect("initialize Silero VAD");
        let silence_frame = vec![0.0_f32; frame_size];

        let events = vad.process_frame(&silence_frame).expect("process frame");
        assert!(
            events
                .iter()
                .any(|e| matches!(e, VadEvent::SpeechProbability { .. }))
        );
        assert_eq!(vad.state(), VadState::Silence);
    }

    #[test]
    fn silero_vad_rejects_unsupported_sample_rate() {
        let config = SileroVadConfig {
            sample_rate: 44100,
            ..SileroVadConfig::default()
        };

        assert!(matches!(
            SileroVad::new(config),
            Err(VadError::UnsupportedSampleRate(44100))
        ));
    }
}
