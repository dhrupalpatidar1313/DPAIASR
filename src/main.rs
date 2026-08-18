use anyhow::{Context, Result};
use dpaiasr::platform::Platform;
use dpaiasr::{
    AsrWorker, AsrWorkerConfig, BackendPreference, BackendResolver, BackendResolverConfig,
    CpuWhisperAsr, ModelManager, ModelManagerConfig,
};
use dpaiasr::{
    AudioCapture, AudioCaptureConfig, CaptureEvent, EnergyVad, PreprocessingConfig, SileroVad,
    SileroVadConfig, Vad, VadConfig, VadEvent,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use dpaiasr::platform::{HotkeyManager, OverlayUI, TargetSnapshot, OverlayState};
use dpaiasr::session::{SessionController, SessionState, SpeechState, ProcessingState, SessionId, UtteranceId};
use dpaiasr::pipeline::AsrJob;
use dpaiasr::injection::{InjectionResult, InjectionWorker, InjectionWorkerConfig};
use global_hotkey::GlobalHotKeyEvent;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{MSG, PeekMessageW, TranslateMessage, DispatchMessageW, PM_REMOVE};
use std::time::Duration;
use tracing::{debug, error, info, warn};
use tracing_subscriber::EnvFilter;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    info!(platform = ?Platform::current(), "starting DPAIASR microphone and VAD pipeline");

    let resolution = BackendResolver::resolve(&BackendResolverConfig {
        preference: BackendPreference::Auto,
        cpu_threads: 6, // i5-9400F has 6 physical cores, no HT
        gpu_device: 0,
    }).map_err(|e| anyhow::anyhow!(e))?;
    info!(backend = ?resolution.backend_kind, reason = %resolution.reason, "backend selected");

    let rt = tokio::runtime::Runtime::new().context("create tokio runtime")?;
    let model_path = rt
        .block_on(async {
            let mgr_config = match resolution.backend_kind {
                #[cfg(feature = "cuda")]
                dpaiasr::asr::backend::AsrBackendKind::Cuda => ModelManagerConfig::default_medium_en(),
                dpaiasr::asr::backend::AsrBackendKind::Cpu => ModelManagerConfig::default_small_q5_1(), // Or tiny_en, user's call. Let's use small_q5_1 as fallback.
            };
            ModelManager::ensure_model(&mgr_config).await
        })
        .context("model download/verification failed")?;

    #[cfg(feature = "cuda")]
    let asr_handle = if resolution.backend_kind == dpaiasr::asr::backend::AsrBackendKind::Cuda {
        let engine = dpaiasr::GpuWhisperAsr::new(&model_path, resolution.gpu_device.unwrap_or(0), resolution.cpu_threads)
            .context("failed to initialize GPU ASR engine")?;
        let (transcript_tx, transcript_rx) = crossbeam_channel::bounded(32);
        let handle = AsrWorker::spawn(engine, AsrWorkerConfig::default(), transcript_tx);
        (handle, transcript_rx)
    } else {
        let engine = CpuWhisperAsr::new(&model_path, resolution.cpu_threads as i32)
            .context("failed to initialize CPU ASR engine")?;
        let (transcript_tx, transcript_rx) = crossbeam_channel::bounded(32);
        let handle = AsrWorker::spawn(engine, AsrWorkerConfig::default(), transcript_tx);
        (handle, transcript_rx)
    };

    #[cfg(not(feature = "cuda"))]
    let asr_handle = {
        let engine = CpuWhisperAsr::new(&model_path, resolution.cpu_threads as i32)
            .context("failed to initialize CPU ASR engine")?;
        let (transcript_tx, transcript_rx) = crossbeam_channel::bounded(32);
        let handle = AsrWorker::spawn(engine, AsrWorkerConfig::default(), transcript_tx);
        (handle, transcript_rx)
    };

    let (asr_handle, transcript_rx) = asr_handle;

    let silero_config = SileroVadConfig::default();
    match SileroVad::new(silero_config) {
        Ok(silero_vad) => {
            info!("initialized Silero VAD (neural network backend)");
            let capture_config = AudioCaptureConfig {
                preprocessing: PreprocessingConfig {
                    target_sample_rate: 16_000,
                    frame_duration_ms: 32,
                },
                ..AudioCaptureConfig::default()
            };
            run_pipeline(capture_config, silero_vad, asr_handle, transcript_rx)
        }
        Err(err) => {
            warn!(error = %err, "Silero VAD unavailable, falling back to EnergyVad");
            let energy_vad = EnergyVad::new(VadConfig::default()).context("creating energy VAD")?;
            run_pipeline(AudioCaptureConfig::default(), energy_vad, asr_handle, transcript_rx)
        }
    }
}

fn run_pipeline<V: Vad + Send + 'static>(
    config: AudioCaptureConfig,
    vad: V,
    asr_handle: dpaiasr::AsrWorkerHandle,
    transcript_rx: crossbeam_channel::Receiver<Result<dpaiasr::pipeline::InjectionJob, dpaiasr::AsrError>>,
) -> Result<()> {
    let capture = AudioCapture::start(config, vad).context("starting microphone capture")?;

    let stopping = Arc::new(AtomicBool::new(false));
    let handler_state = Arc::clone(&stopping);
    ctrlc::set_handler(move || handler_state.store(true, Ordering::Release))
        .context("installing Ctrl+C handler")?;

    info!("microphone capture is running; press Ctrl+C to stop");
    
    let mut session = SessionController::new();
    let hotkey = HotkeyManager::new().context("failed to register hotkey")?;
    let overlay = OverlayUI::new().context("failed to initialize overlay UI")?;

    let current_session_id = SessionId(1);

    let (inj_tx, inj_rx) = crossbeam_channel::bounded(32);
    let injection_handle = InjectionWorker::spawn(InjectionWorkerConfig::default(), inj_tx);

    info!("Phase 2A Dictation ready: Press Alt+M to toggle listening");

    while !stopping.load(Ordering::Acquire) {
        #[cfg(target_os = "windows")]
        {
            // Pump the Win32 message queue on the main thread so WM_HOTKEY
            // messages dispatched by RegisterHotKey are delivered to
            // GlobalHotKeyEvent::receiver().
            let mut msg = MSG::default();
            unsafe {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).into() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
        if let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            if event.state == global_hotkey::HotKeyState::Pressed {
                if hotkey.is_toggle(&event) {
                    let new_session = session.toggle();
                    if new_session == SessionState::Stopped {
                        session.set_speech(SpeechState::Silent);
                        // Do not reset processing state to allow pending jobs to finish safely
                    }
                    update_overlay(&overlay, &session);
                    match new_session {
                        SessionState::Stopped => info!("Dictation STOPPED"),
                        SessionState::Listening => info!("Dictation STARTED (Listening)"),
                        _ => {}
                    }
                } else if hotkey.is_retry(&event) {
                    info!("Alt+R pressed: Triggering pending queue retry");
                    if let Err(e) = injection_handle.retry_pending() {
                        error!("Failed to send retry command: {:?}", e);
                    }
                } else if hotkey.is_copy(&event) {
                    info!("Alt+C pressed: Copying latest orphaned transcript to clipboard");
                    if let Err(e) = injection_handle.copy_orphaned_to_clipboard() {
                        error!("Failed to send copy orphaned command: {:?}", e);
                    }
                }
            }
        }
        if let Ok(event) = capture.events().recv_timeout(Duration::from_millis(50)) {
            match event {
                CaptureEvent::Vad(VadEvent::AudioLevel { rms, dbfs }) => {
                    debug!(rms, dbfs, "VAD audio level");
                }
                CaptureEvent::Vad(VadEvent::SpeechProbability { probability }) => {
                    debug!(probability, "speech probability");
                }
                CaptureEvent::Vad(VadEvent::StateChanged(dpaiasr::vad::VadState::Speaking)) => {
                    if session.session == SessionState::Listening {
                        info!("speech started (VAD triggered)");
                        session.set_speech(SpeechState::Speaking);
                        update_overlay(&overlay, &session);
                    }
                }
                CaptureEvent::Vad(VadEvent::Utterance { id, audio, .. }) => {
                    let ut_id = UtteranceId(id);
                    let target = TargetSnapshot::current();
                    
                    let job = AsrJob {
                        session_id: current_session_id,
                        utterance_id: ut_id,
                        audio: audio.clone(),
                        target,
                    };

                    if session.session == SessionState::Listening {
                        info!("utterance detected ({} samples)", audio.len());
                        session.set_speech(SpeechState::Silent);
                        if session.processing == ProcessingState::Idle {
                            session.set_processing(ProcessingState::Transcribing);
                        }
                        update_overlay(&overlay, &session);
                        if let Err(e) = asr_handle.enqueue(job) {
                            warn!(error = %e, "ASR queue full; utterance dropped until queue drains");
                        }
                    } else {
                        debug!("ignored utterance (session inactive)");
                    }
                    #[cfg(not(target_os = "windows"))]
                    {
                        info!("utterance detected ({} samples)", audio.len());
                        if let Err(e) = asr_handle.enqueue(job) {
                            warn!(error = %e, "ASR queue full; utterance dropped until queue drains");
                        }
                    }
                }
                CaptureEvent::Vad(_event) => {} // Suppress noisy debug logs for unhandled VAD events
                CaptureEvent::Warning(message) => error!(message, "capture warning"),
                CaptureEvent::StreamError(message) => {
                    error!(message, "microphone stream stopped or became unavailable");
                }
                CaptureEvent::Stopped => break,
            }
        }

        while let Ok(result) = transcript_rx.try_recv() {
            match result {
                Ok(inj_job) => {
                    info!(
                        segments = inj_job.transcript.segments.len(),
                        text = %inj_job.transcript.text,
                        "transcription complete, enqueueing to InjectionWorker"
                    );

                    if let Err(e) = injection_handle.enqueue(inj_job) {
                        error!("Injection queue full or worker dead: {:?}", e);
                    }
                }
                Err(e) => {
                    error!(error = %e, "ASR error");
                }
            }
        }

        while let Ok(inj_result) = inj_rx.try_recv() {
            match inj_result {
                InjectionResult::Started(_sid, _uid) => {
                    session.set_processing(ProcessingState::Injecting);
                    update_overlay(&overlay, &session);
                }
                InjectionResult::Succeeded(_sid, _uid) => {
                    info!("Injection succeeded");
                    session.set_processing(ProcessingState::Idle);
                    update_overlay(&overlay, &session);
                }
                InjectionResult::Pending(_sid, _uid) => {
                    info!("Injection pending due to focus loss");
                    session.set_processing(ProcessingState::WaitingForTarget);
                    update_overlay(&overlay, &session);
                }
                InjectionResult::Orphaned(_sid, _uid) => {
                    warn!("Target window was closed; transcript stored in orphaned recovery queue (Press Alt+C to copy)");
                    session.set_processing(ProcessingState::Orphaned);
                    update_overlay(&overlay, &session);
                }
                InjectionResult::CopiedToClipboard(_sid, _uid, text) => {
                    info!(text = %text, "Copied orphaned transcript to clipboard");
                    session.set_processing(ProcessingState::Idle);
                    update_overlay(&overlay, &session);
                }
                InjectionResult::Failed(_sid, _uid, err) => {
                    error!("Injection failed: {}", err);
                    session.set_processing(ProcessingState::Error);
                    update_overlay(&overlay, &session);
                }
                InjectionResult::Retryable(_sid, _uid, err) => {
                    warn!("Injection retryable: {}", err);
                }
            }
        }
    }

    capture.stop().context("stopping microphone capture")
}

fn update_overlay(overlay: &OverlayUI, session: &SessionController) {
    let state = match (session.session, session.speech, session.processing) {
        // High priority error
        (_, _, ProcessingState::Error) => OverlayState::Error,
        // High priority orphaned / waiting
        (_, _, ProcessingState::Orphaned) => OverlayState::Orphaned,
        (_, _, ProcessingState::WaitingForTarget) => OverlayState::WaitingForTarget,
        // Processing
        (_, _, ProcessingState::Injecting) => OverlayState::Injecting,
        (_, _, ProcessingState::Transcribing) => OverlayState::Transcribing,
        // Muted
        (SessionState::Muted, _, _) => OverlayState::Muted,
        // Active recording
        (SessionState::Listening, SpeechState::Speaking, _) => OverlayState::Speaking,
        (SessionState::Listening, SpeechState::Silent, _) => OverlayState::Listening,
        // Stopping / Stopped
        (SessionState::Stopping, _, _) => OverlayState::Stopping,
        (SessionState::Stopped, _, _) => OverlayState::Stopped,
    };
    overlay.set_state(state);
}
