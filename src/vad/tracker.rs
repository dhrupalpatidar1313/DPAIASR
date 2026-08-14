//! Generic utterance segmentation and state tracking for VAD engines.

use std::collections::VecDeque;

use super::{VadConfig, VadError, VadEvent, VadState};

/// Segmenter that converts frame-by-frame speech decisions into continuous utterances.
///
/// Handles:
/// - Pre-roll buffering (retaining frames leading up to speech confirmation)
/// - Start confirmation frames (avoiding false positives from brief noises)
/// - Hangover frames (preventing mid-sentence clipping on short pauses)
/// - Maximum utterance duration limits
/// - Utterance generation with unique sequential IDs
#[derive(Debug)]
pub struct UtteranceTracker {
    config: VadConfig,
    frame_size: usize,
    state: VadState,
    speech_confirmation: usize,
    silent_frames: usize,
    utterance_id: u64,
    utterance: Vec<f32>,
    pre_roll: VecDeque<Vec<f32>>,
}

impl UtteranceTracker {
    /// Creates a new [`UtteranceTracker`] configured with the given [`VadConfig`].
    ///
    /// # Errors
    ///
    /// Returns [`VadError::InvalidFrameConfiguration`] if sample rate and frame duration
    /// do not produce a valid integer frame size.
    pub fn new(config: VadConfig) -> Result<Self, VadError> {
        let frame_size = config.frame_size()?;
        Ok(Self {
            config,
            frame_size,
            state: VadState::Silence,
            speech_confirmation: 0,
            silent_frames: 0,
            utterance_id: 0,
            utterance: Vec::new(),
            pre_roll: VecDeque::with_capacity(config.pre_roll_frames),
        })
    }

    /// Returns the current VAD state (`Silence` or `Speaking`).
    #[inline]
    pub fn state(&self) -> VadState {
        self.state
    }

    /// Returns the expected frame size in samples.
    #[inline]
    pub fn frame_size(&self) -> usize {
        self.frame_size
    }

    /// Returns a reference to the tracker's configuration.
    #[inline]
    pub fn config(&self) -> &VadConfig {
        &self.config
    }

    /// Returns the last assigned utterance ID.
    #[inline]
    pub fn utterance_id(&self) -> u64 {
        self.utterance_id
    }

    /// Processes a single frame and speech classification decision, appending any resulting events.
    pub fn process_frame(&mut self, frame: &[f32], is_speech: bool, events: &mut Vec<VadEvent>) {
        match self.state {
            VadState::Silence if is_speech => {
                self.speech_confirmation += 1;
                self.pre_roll.push_back(frame.to_vec());
                while self.pre_roll.len() > self.config.pre_roll_frames {
                    self.pre_roll.pop_front();
                }
                if self.speech_confirmation >= self.config.start_confirmation_frames {
                    self.state = VadState::Speaking;
                    self.utterance.extend(self.pre_roll.drain(..).flatten());
                    self.silent_frames = 0;
                    events.push(VadEvent::StateChanged(VadState::Speaking));
                }
            }
            VadState::Silence => {
                self.speech_confirmation = 0;
                self.pre_roll.clear();
            }
            VadState::Speaking if is_speech => {
                self.silent_frames = 0;
                self.utterance.extend_from_slice(frame);
                if self.utterance.len() / self.frame_size >= self.config.maximum_utterance_frames {
                    self.finish_utterance(events);
                }
            }
            VadState::Speaking => {
                self.silent_frames += 1;
                self.utterance.extend_from_slice(frame);
                if self.silent_frames >= self.config.hangover_frames {
                    self.finish_utterance(events);
                }
            }
        }
    }

    /// Finalizes the current utterance if it meets the minimum duration requirement,
    /// emits the [`VadEvent::Utterance`] and resets state back to `Silence`.
    pub fn finish_utterance(&mut self, events: &mut Vec<VadEvent>) {
        let duration_frames = self.utterance.len() / self.frame_size;
        if duration_frames >= self.config.minimum_utterance_frames {
            self.utterance_id = self.utterance_id.saturating_add(1);
            events.push(VadEvent::Utterance {
                id: self.utterance_id,
                audio: std::mem::take(&mut self.utterance),
                duration_frames,
            });
        } else {
            self.utterance.clear();
        }
        self.state = VadState::Silence;
        self.silent_frames = 0;
        self.speech_confirmation = 0;
        events.push(VadEvent::StateChanged(VadState::Silence));
    }

    /// Clears any buffered speech or pre-roll and resets the tracker state to `Silence`.
    pub fn reset(&mut self) {
        self.state = VadState::Silence;
        self.speech_confirmation = 0;
        self.silent_frames = 0;
        self.utterance.clear();
        self.pre_roll.clear();
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
    fn tracker_transitions_and_emits_utterance() {
        let config = test_config();
        let frame_size = config.frame_size().expect("valid frame size");
        let mut tracker = UtteranceTracker::new(config).expect("valid tracker");
        let frame = vec![0.1; frame_size];
        let mut events = Vec::new();

        tracker.process_frame(&frame, true, &mut events);
        assert_eq!(tracker.state(), VadState::Silence);
        assert!(!events.contains(&VadEvent::StateChanged(VadState::Speaking)));

        tracker.process_frame(&frame, true, &mut events);
        assert_eq!(tracker.state(), VadState::Speaking);
        assert!(events.contains(&VadEvent::StateChanged(VadState::Speaking)));

        tracker.process_frame(&frame, false, &mut events);
        assert_eq!(tracker.state(), VadState::Speaking);

        tracker.process_frame(&frame, false, &mut events);
        assert_eq!(tracker.state(), VadState::Silence);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, VadEvent::Utterance { .. }))
        );
    }
}
