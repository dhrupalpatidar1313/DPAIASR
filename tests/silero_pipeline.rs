use dpaiasr::{SileroVad, SileroVadConfig, Vad, VadError, VadEvent, VadState};

#[test]
fn silero_vad_processes_silence_without_false_positives() {
    let config = SileroVadConfig {
        start_confirmation_frames: 2,
        hangover_frames: 2,
        minimum_utterance_frames: 2,
        pre_roll_frames: 1,
        ..SileroVadConfig::default()
    };

    let mut vad = SileroVad::new(config).expect("SileroVad should initialize from assets/models");
    let frame_size = vad.config().frame_size().expect("valid frame size");
    assert_eq!(frame_size, 512);

    let silence_frame = vec![0.0_f32; frame_size];

    for _ in 0..10 {
        let events = vad.process_frame(&silence_frame).expect("frame processed");
        assert_eq!(vad.state(), VadState::Silence);
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, VadEvent::StateChanged(VadState::Speaking)))
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, VadEvent::Utterance { .. }))
        );
    }
}

#[test]
fn silero_vad_rejects_invalid_frame_sizes() {
    let config = SileroVadConfig::default();
    let mut vad = SileroVad::new(config).expect("SileroVad should initialize");

    assert!(matches!(vad.process_frame(&[]), Err(VadError::EmptyFrame)));

    assert!(matches!(
        vad.process_frame(&[0.0_f32; 100]),
        Err(VadError::InvalidFrameSize {
            actual: 100,
            expected: 512
        })
    ));
}

#[test]
fn silero_vad_resets_internal_states() {
    let config = SileroVadConfig::default();
    let mut vad = SileroVad::new(config).expect("SileroVad should initialize");
    let frame_size = vad.config().frame_size().expect("valid frame size");

    let frame = vec![0.0_f32; frame_size];
    let _ = vad.process_frame(&frame).expect("frame processed");

    vad.reset();
    assert_eq!(vad.state(), VadState::Silence);
}
