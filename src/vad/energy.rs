//! Deterministic RMS-energy VAD for development and testing.

use super::tracker::UtteranceTracker;
use super::{Vad, VadConfig, VadError, VadEvent, VadState};

/// Voice activity detector based on RMS energy and decibel thresholding.
#[derive(Debug)]
pub struct EnergyVad {
    config: VadConfig,
    tracker: UtteranceTracker,
}

impl EnergyVad {
    /// Creates a new [`EnergyVad`] instance with the provided [`VadConfig`].
    ///
    /// # Errors
    ///
    /// Returns [`VadError::InvalidFrameConfiguration`] if the sample rate and frame duration
    /// cannot produce an integer frame size.
    pub fn new(config: VadConfig) -> Result<Self, VadError> {
        let tracker = UtteranceTracker::new(config)?;
        Ok(Self { config, tracker })
    }

    /// Returns a reference to the tracker configuration.
    #[inline]
    pub fn config(&self) -> &VadConfig {
        &self.config
    }

    /// Returns the current VAD state.
    #[inline]
    pub fn state(&self) -> VadState {
        self.tracker.state()
    }
}

impl Vad for EnergyVad {
    fn process_frame(&mut self, frame: &[f32]) -> Result<Vec<VadEvent>, VadError> {
        if frame.is_empty() {
            return Err(VadError::EmptyFrame);
        }
        let expected_size = self.tracker.frame_size();
        if frame.len() != expected_size {
            return Err(VadError::InvalidFrameSize {
                actual: frame.len(),
                expected: expected_size,
            });
        }

        let rms =
            (frame.iter().map(|sample| sample * sample).sum::<f32>() / frame.len() as f32).sqrt();
        let dbfs = 20.0 * rms.max(f32::EPSILON).log10();
        let is_speech = match self.tracker.state() {
            VadState::Silence => dbfs >= self.config.start_threshold_dbfs,
            VadState::Speaking => dbfs >= self.config.end_threshold_dbfs,
        };

        let mut events = vec![VadEvent::AudioLevel { rms, dbfs }];
        self.tracker.process_frame(frame, is_speech, &mut events);
        Ok(events)
    }

    fn reset(&mut self) {
        self.tracker.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> VadConfig {
        VadConfig {
            start_confirmation_frames: 2,
            hangover_frames: 2,
            minimum_utterance_frames: 2,
            pre_roll_frames: 1,
            ..VadConfig::default()
        }
    }

    #[test]
    fn energy_vad_requires_confirmation_and_emits_one_utterance() {
        let config = test_config();
        let frame_size = config.frame_size().expect("valid frame size");
        let mut vad = EnergyVad::new(config).expect("valid VAD");
        let speech = vec![0.2; frame_size];
        let silence = vec![0.0; frame_size];

        assert!(
            vad.process_frame(&speech)
                .expect("frame accepted")
                .iter()
                .all(|event| { !matches!(event, VadEvent::StateChanged(VadState::Speaking)) })
        );
        let start = vad.process_frame(&speech).expect("frame accepted");
        assert!(start.contains(&VadEvent::StateChanged(VadState::Speaking)));
        vad.process_frame(&silence).expect("frame accepted");
        let end = vad.process_frame(&silence).expect("frame accepted");
        assert!(
            end.iter()
                .any(|event| matches!(event, VadEvent::Utterance { .. }))
        );
    }

    #[test]
    fn energy_vad_rejects_wrong_frame_size() {
        let mut vad = EnergyVad::new(VadConfig::default()).expect("valid VAD");
        assert!(matches!(
            vad.process_frame(&[0.0; 10]),
            Err(VadError::InvalidFrameSize { .. })
        ));
    }
}
