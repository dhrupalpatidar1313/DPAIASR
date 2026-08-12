//! Deterministic RMS-energy VAD for development and testing.

use std::collections::VecDeque;

use super::{Vad, VadConfig, VadError, VadEvent, VadState};

#[derive(Debug)]
pub struct EnergyVad {
    config: VadConfig,
    frame_size: usize,
    state: VadState,
    speech_confirmation: usize,
    silent_frames: usize,
    utterance_id: u64,
    utterance: Vec<f32>,
    pre_roll: VecDeque<Vec<f32>>,
}

impl EnergyVad {
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

    fn finish_utterance(&mut self, events: &mut Vec<VadEvent>) {
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
        events.push(VadEvent::StateChanged(VadState::Silence));
    }
}

impl Vad for EnergyVad {
    fn process_frame(&mut self, frame: &[f32]) -> Result<Vec<VadEvent>, VadError> {
        if frame.is_empty() {
            return Err(VadError::EmptyFrame);
        }
        if frame.len() != self.frame_size {
            return Err(VadError::InvalidFrameSize {
                actual: frame.len(),
                expected: self.frame_size,
            });
        }

        let rms =
            (frame.iter().map(|sample| sample * sample).sum::<f32>() / frame.len() as f32).sqrt();
        let dbfs = 20.0 * rms.max(f32::EPSILON).log10();
        let is_speech = match self.state {
            VadState::Silence => dbfs >= self.config.start_threshold_dbfs,
            VadState::Speaking => dbfs >= self.config.end_threshold_dbfs,
        };

        let mut events = vec![VadEvent::AudioLevel { rms, dbfs }];
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
                    self.finish_utterance(&mut events);
                }
            }
            VadState::Speaking => {
                self.silent_frames += 1;
                self.utterance.extend_from_slice(frame);
                if self.silent_frames >= self.config.hangover_frames {
                    self.finish_utterance(&mut events);
                }
            }
        }
        Ok(events)
    }

    fn reset(&mut self) {
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
