use dpaiasr::{AsrEngine, CpuWhisperAsr, ModelManager, ModelManagerConfig};

#[tokio::test]
#[ignore = "requires downloaded model; run with: cargo test -- --ignored"]
async fn test_cpu_whisper_transcribes_test_audio_wav() {
    let config = ModelManagerConfig::default_small_q5_1();
    let model_path = ModelManager::ensure_model(&config)
        .await
        .expect("failed to download/verify model");

    let mut engine = CpuWhisperAsr::new(&model_path, 4).expect("failed to create engine");

    // We need some wav parsing here to actually use assets/test_audio.wav
    // Since we added hound as a dev-dependency:
    let wav_path = std::path::Path::new("assets/test_audio.wav");
    if !wav_path.exists() {
        eprintln!("assets/test_audio.wav not found, skipping real transcription test");
        return;
    }

    let mut reader = hound::WavReader::open(wav_path).unwrap();
    let spec = reader.spec();
    // If the audio isn't 16kHz or 1-channel, we do a very naive linear resampling/mixing
    // just so the test can run. Whisper requires 16kHz.
    let src_sample_rate = spec.sample_rate as f32;

    let raw_samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => reader
            .samples::<i16>()
            .map(|s| s.unwrap() as f32 / 32768.0)
            .collect(),
        hound::SampleFormat::Float => reader.samples::<f32>().map(|s| s.unwrap()).collect(),
    };

    let channels = spec.channels as usize;
    let mut mono_samples = Vec::with_capacity(raw_samples.len() / channels);
    for chunk in raw_samples.chunks(channels) {
        let sum: f32 = chunk.iter().sum();
        mono_samples.push(sum / channels as f32);
    }

    // Naive linear resample
    let mut samples = Vec::new();
    let ratio = src_sample_rate / 16000.0;
    let new_len = (mono_samples.len() as f32 / ratio) as usize;
    for i in 0..new_len {
        let src_idx = (i as f32 * ratio) as usize;
        if src_idx < mono_samples.len() {
            samples.push(mono_samples[src_idx]);
        }
    }

    let started = std::time::Instant::now();
    let transcript = engine.transcribe(&samples).expect("transcription failed");
    let elapsed = started.elapsed();

    println!("Transcript: {}", transcript.text);
    println!("Elapsed: {:?}", elapsed);

    let duration_secs = samples.len() as f32 / 16000.0;
    let rtf = elapsed.as_secs_f32() / duration_secs;

    // This bound is NOT the production latency target (<1.5 s end-to-end).
    // It is a loose sanity check to detect catastrophic regression.
    assert!(
        rtf < 3.0,
        "RTF {rtf:.2} suggests catastrophic slowness — investigate model/thread config"
    );
    assert!(!transcript.text.is_empty(), "transcript should not be empty");
}
