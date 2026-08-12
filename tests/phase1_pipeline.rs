use dpaiasr::{EnergyVad, Vad, VadConfig, VadEvent, VadState};

#[test]
fn public_vad_pipeline_emits_an_ordered_utterance() {
    let config = VadConfig {
        start_confirmation_frames: 2,
        hangover_frames: 2,
        minimum_utterance_frames: 2,
        pre_roll_frames: 1,
        ..VadConfig::default()
    };
    let frame_size = config
        .frame_size()
        .expect("default frame configuration is valid");
    let mut vad = EnergyVad::new(config).expect("default VAD configuration is valid");
    let speech = vec![0.2; frame_size];
    let silence = vec![0.0; frame_size];

    let first = vad
        .process_frame(&speech)
        .expect("speech frame is accepted");
    assert!(!first.contains(&VadEvent::StateChanged(VadState::Speaking)));
    vad.process_frame(&speech)
        .expect("speech frame is accepted");
    vad.process_frame(&silence)
        .expect("silence frame is accepted");
    let final_events = vad
        .process_frame(&silence)
        .expect("silence frame is accepted");

    let utterance = final_events
        .into_iter()
        .find_map(|event| match event {
            VadEvent::Utterance {
                id,
                audio,
                duration_frames,
            } => Some((id, audio, duration_frames)),
            _ => None,
        })
        .expect("VAD emits a completed utterance");

    assert_eq!(utterance.0, 1);
    assert_eq!(utterance.2, 3);
    assert_eq!(utterance.1.len(), frame_size * utterance.2);
}
