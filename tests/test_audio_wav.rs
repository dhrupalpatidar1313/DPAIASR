use dpaiasr::{
    AsrJob, AsrWorker, AsrWorkerConfig, AudioPreprocessor, NullAsr, PreprocessingConfig, SileroVad,
    SileroVadConfig, Vad, VadEvent,
};
use dpaiasr::session::{SessionId, UtteranceId};
use std::path::Path;

#[test]
fn test_audio_file_processing_with_silero_vad_and_asr() {
    let wav_path = Path::new("assets/test_audio.wav");
    if !wav_path.exists() {
        eprintln!("test_audio.wav not found, skipping test");
        return;
    }

    let mut reader = hound::WavReader::open(wav_path).expect("open test_audio.wav");
    let spec = reader.spec();
    println!(
        "Loaded WAV: sample_rate={}Hz, channels={}, bits_per_sample={}, format={:?}",
        spec.sample_rate, spec.channels, spec.bits_per_sample, spec.sample_format
    );

    let samples: Vec<f32> = match (spec.sample_format, spec.bits_per_sample) {
        (hound::SampleFormat::Int, 16) => reader
            .samples::<i16>()
            .map(|s| s.expect("sample") as f32 / i16::MAX as f32)
            .collect(),
        (hound::SampleFormat::Int, 32) => reader
            .samples::<i32>()
            .map(|s| s.expect("sample") as f32 / i32::MAX as f32)
            .collect(),
        (hound::SampleFormat::Float, 32) => reader
            .samples::<f32>()
            .map(|s| s.expect("sample"))
            .collect(),
        (format, bits) => panic!("Unsupported WAV format: {:?} {}bit", format, bits),
    };

    println!("Total raw samples read: {}", samples.len());

    let preprocessor_config = PreprocessingConfig {
        target_sample_rate: 16_000,
        frame_duration_ms: 32,
    };
    let mut preprocessor = AudioPreprocessor::new(
        spec.sample_rate,
        spec.channels as usize,
        preprocessor_config,
    )
    .expect("create audio preprocessor");

    let silero_config = SileroVadConfig::default();
    let mut vad = SileroVad::new(silero_config).expect("create Silero VAD");

    let (transcript_tx, transcript_rx) = crossbeam_channel::bounded(32);
    let asr_handle = AsrWorker::spawn(NullAsr, AsrWorkerConfig::default(), transcript_tx);

    let chunk_size = 4096;
    let mut utterances = Vec::new();

    for chunk in samples.chunks(chunk_size) {
        let frames = preprocessor.push_interleaved(chunk);
        for frame in frames {
            let events = vad
                .process_frame(&frame)
                .expect("process frame through Silero VAD");
            for event in events {
                if let VadEvent::Utterance { id, audio, .. } = event {
                    println!("  [Utterance #{}] sent to ASR queue", id);
                    utterances.push(id);
                    let job = AsrJob {
                        session_id: SessionId(1),
                        utterance_id: UtteranceId(id),
                        audio,
                        target: None,
                    };
                    asr_handle.enqueue(job).expect("enqueue failed");
                }
            }
        }
    }

    // Drain transcript queue
    let mut received_transcripts = 0;
    while let Ok(_result) = transcript_rx.recv_timeout(std::time::Duration::from_millis(500)) {
        received_transcripts += 1;
        println!("Received transcript from ASR worker");
    }

    assert_eq!(
        utterances.len(),
        received_transcripts,
        "Should receive one transcript per utterance"
    );
}
