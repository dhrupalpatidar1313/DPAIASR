use dpaiasr::{AudioPreprocessor, PreprocessingConfig, SileroVad, SileroVadConfig, Vad, VadEvent};
use std::path::Path;

#[test]
fn test_audio_file_processing_with_silero_vad() {
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

    // Convert samples to normalized f32
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
    let duration_secs = samples.len() as f32 / (spec.sample_rate as f32 * spec.channels as f32);
    println!("Audio duration: {:.2} seconds", duration_secs);

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

    let silero_config = SileroVadConfig {
        sample_rate: 16_000,
        start_threshold: 0.5,
        end_threshold: 0.35,
        ..SileroVadConfig::default()
    };
    let mut vad = SileroVad::new(silero_config).expect("create Silero VAD");

    let chunk_size = 4096;
    let mut total_frames = 0;
    let mut speech_frames = 0;
    let mut utterances = Vec::new();

    for chunk in samples.chunks(chunk_size) {
        let frames = preprocessor.push_interleaved(chunk);
        for frame in frames {
            total_frames += 1;
            let events = vad
                .process_frame(&frame)
                .expect("process frame through Silero VAD");
            for event in events {
                match event {
                    VadEvent::SpeechProbability { probability } if probability >= 0.5 => {
                        speech_frames += 1;
                    }
                    VadEvent::Utterance {
                        id,
                        audio,
                        duration_frames,
                    } => {
                        println!(
                            "  [Utterance #{}] Duration: {} frames ({:.2}s), audio samples: {}",
                            id,
                            duration_frames,
                            duration_frames as f32 * 0.032,
                            audio.len()
                        );
                        utterances.push((id, duration_frames, audio.len()));
                    }
                    _ => {}
                }
            }
        }
    }

    println!(
        "Processed {} frames ({:.2}s), speech frames detected: {} ({:.1}%), utterances found: {}",
        total_frames,
        total_frames as f32 * 0.032,
        speech_frames,
        if total_frames > 0 {
            (speech_frames as f32 / total_frames as f32) * 100.0
        } else {
            0.0
        },
        utterances.len()
    );

    assert!(total_frames > 0, "Frames should have been processed");
}
