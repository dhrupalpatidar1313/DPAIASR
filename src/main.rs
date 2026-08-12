use anyhow::{Context, Result};
use dpaiasr::platform::Platform;
use dpaiasr::{AudioCapture, AudioCaptureConfig, CaptureEvent, EnergyVad, VadConfig, VadEvent};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tracing::{debug, error, info};
use tracing_subscriber::EnvFilter;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    info!(platform = ?Platform::current(), "starting phase 1 microphone and energy VAD pipeline");
    let vad = EnergyVad::new(VadConfig::default()).context("creating energy VAD")?;
    let capture = AudioCapture::start(AudioCaptureConfig::default(), vad)
        .context("starting microphone capture")?;

    let stopping = Arc::new(AtomicBool::new(false));
    let handler_state = Arc::clone(&stopping);
    ctrlc::set_handler(move || handler_state.store(true, Ordering::Release))
        .context("installing Ctrl+C handler")?;

    info!("microphone capture is running; press Ctrl+C to stop");
    while !stopping.load(Ordering::Acquire) {
        if let Ok(event) = capture.events().recv_timeout(Duration::from_millis(100)) {
            match event {
                CaptureEvent::Vad(VadEvent::AudioLevel { rms, dbfs }) => {
                    debug!(rms, dbfs, "VAD audio level")
                }
                CaptureEvent::Vad(event) => info!(?event, "VAD event"),
                CaptureEvent::Warning(message) => error!(message, "capture warning"),
                CaptureEvent::StreamError(message) => {
                    error!(message, "microphone stream stopped or became unavailable")
                }
                CaptureEvent::Stopped => break,
            }
        }
    }

    capture.stop().context("stopping microphone capture")
}
