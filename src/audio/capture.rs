//! Microphone capture and the dedicated preprocessing/VAD worker.

use std::thread::{self, JoinHandle};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, Stream, StreamConfig};
use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use thiserror::Error;

use super::preprocess::{AudioPreprocessor, PreprocessingConfig};
use crate::vad::{Vad, VadError, VadEvent};

const AUDIO_CHANNEL_CAPACITY: usize = 32;

#[derive(Debug, Clone, Copy)]
pub struct AudioCaptureConfig {
    pub preprocessing: PreprocessingConfig,
    pub channel_capacity: usize,
}

impl Default for AudioCaptureConfig {
    fn default() -> Self {
        Self {
            preprocessing: PreprocessingConfig::default(),
            channel_capacity: AUDIO_CHANNEL_CAPACITY,
        }
    }
}

#[derive(Debug)]
pub enum CaptureEvent {
    Vad(VadEvent),
    Warning(String),
    StreamError(String),
    Stopped,
}

#[derive(Debug, Error)]
pub enum AudioCaptureError {
    #[error("no default input device is available")]
    NoInputDevice,
    #[error("failed to query the default input configuration")]
    DefaultConfig(#[source] cpal::DefaultStreamConfigError),
    #[error("failed to build the input stream")]
    BuildStream(#[source] cpal::BuildStreamError),
    #[error("failed to start the input stream")]
    PlayStream(#[source] cpal::PlayStreamError),
    #[error("invalid capture channel capacity")]
    InvalidChannelCapacity,
    #[error("unsupported sample format {0:?}")]
    UnsupportedSampleFormat(SampleFormat),
    #[error("preprocessing failed: {0}")]
    Preprocessing(#[from] super::preprocess::PreprocessingError),
    #[error("VAD failed: {0}")]
    Vad(#[from] VadError),
    #[error("capture worker panicked")]
    WorkerPanicked,
}

struct RawAudio {
    samples: Vec<f32>,
}

pub struct AudioCapture {
    events: Receiver<CaptureEvent>,
    stop_sender: Option<Sender<()>>,
    stream: Option<Stream>,
    worker: Option<JoinHandle<()>>,
}

impl AudioCapture {
    pub fn start<V: Vad + Send + 'static>(
        config: AudioCaptureConfig,
        vad: V,
    ) -> Result<Self, AudioCaptureError> {
        if config.channel_capacity == 0 {
            return Err(AudioCaptureError::InvalidChannelCapacity);
        }

        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or(AudioCaptureError::NoInputDevice)?;
        let supported = device
            .default_input_config()
            .map_err(AudioCaptureError::DefaultConfig)?;
        let sample_format = supported.sample_format();
        let stream_config: StreamConfig = supported.into();
        let channels = usize::from(stream_config.channels);
        let source_rate = stream_config.sample_rate.0;
        tracing::info!(
            device = %device.name().unwrap_or_else(|_| "unknown".to_owned()),
            ?sample_format,
            channels,
            sample_rate = source_rate,
            "selected microphone input"
        );
        let preprocessor = AudioPreprocessor::new(source_rate, channels, config.preprocessing)?;

        let (raw_sender, raw_receiver) = bounded::<RawAudio>(config.channel_capacity);
        let (event_sender, event_receiver) = bounded::<CaptureEvent>(config.channel_capacity);
        let (stop_sender, stop_receiver) = bounded(1);
        let worker_event_sender = event_sender.clone();
        let worker = thread::Builder::new()
            .name("dpaiasr-audio-vad".to_owned())
            .spawn(move || {
                run_worker(
                    raw_receiver,
                    stop_receiver,
                    worker_event_sender,
                    preprocessor,
                    vad,
                )
            })
            .map_err(|_| AudioCaptureError::WorkerPanicked)?;
        tracing::info!("audio worker started");

        let stream = build_stream(
            &device,
            &stream_config,
            sample_format,
            raw_sender,
            event_sender.clone(),
        )?;
        tracing::info!("microphone stream created");
        stream.play().map_err(AudioCaptureError::PlayStream)?;
        tracing::info!("microphone stream playback started");

        Ok(Self {
            events: event_receiver,
            stop_sender: Some(stop_sender),
            stream: Some(stream),
            worker: Some(worker),
        })
    }

    pub fn events(&self) -> &Receiver<CaptureEvent> {
        &self.events
    }

    pub fn stop(mut self) -> Result<(), AudioCaptureError> {
        self.stop_sender.take();
        self.stream.take();
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| AudioCaptureError::WorkerPanicked)?;
        }
        Ok(())
    }
}

fn build_stream(
    device: &cpal::Device,
    config: &StreamConfig,
    format: SampleFormat,
    sender: Sender<RawAudio>,
    event_sender: Sender<CaptureEvent>,
) -> Result<Stream, AudioCaptureError> {
    let error_callback = move |error: cpal::StreamError| {
        let message = error.to_string();
        tracing::error!(error = %message, "microphone stream error");
        let _ = event_sender.try_send(CaptureEvent::StreamError(message));
    };

    match format {
        SampleFormat::F32 => build_input_stream::<f32>(device, config, sender, error_callback),
        SampleFormat::I16 => build_input_stream::<i16>(device, config, sender, error_callback),
        SampleFormat::U16 => build_input_stream::<u16>(device, config, sender, error_callback),
        other => Err(AudioCaptureError::UnsupportedSampleFormat(other)),
    }
}

fn build_input_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    sender: Sender<RawAudio>,
    error_callback: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<Stream, AudioCaptureError>
where
    T: Sample + FromSample<f32> + cpal::SizedSample,
    f32: FromSample<T>,
{
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                let samples = data
                    .iter()
                    .map(|sample| f32::from_sample(*sample))
                    .collect();
                match sender.try_send(RawAudio { samples }) {
                    Ok(()) => {}
                    Err(TrySendError::Full(_)) => {
                        tracing::warn!(
                            "audio capture channel is full; dropping one callback buffer"
                        );
                    }
                    Err(TrySendError::Disconnected(_)) => {}
                }
            },
            error_callback,
            None,
        )
        .map_err(AudioCaptureError::BuildStream)
}

fn run_worker<V: Vad>(
    raw_receiver: Receiver<RawAudio>,
    stop_receiver: Receiver<()>,
    event_sender: Sender<CaptureEvent>,
    mut preprocessor: AudioPreprocessor,
    mut vad: V,
) {
    tracing::info!("audio worker entered processing loop");
    let mut raw_buffers = 0_u64;
    let mut raw_samples = 0_u64;
    let mut processed_frames = 0_u64;
    let diagnostics = crossbeam_channel::tick(Duration::from_secs(1));

    loop {
        crossbeam_channel::select! {
            recv(stop_receiver) -> _ => break,
            recv(raw_receiver) -> raw => {
                let Ok(raw) = raw else { break };
                raw_buffers = raw_buffers.saturating_add(1);
                raw_samples = raw_samples.saturating_add(raw.samples.len() as u64);
                for frame in preprocessor.push_interleaved(&raw.samples) {
                    processed_frames = processed_frames.saturating_add(1);
                    match vad.process_frame(&frame) {
                        Ok(events) => {
                            for event in events {
                                if event_sender.send(CaptureEvent::Vad(event)).is_err() {
                                    return;
                                }
                            }
                        }
                        Err(error) => {
                            let _ = event_sender.send(CaptureEvent::Warning(error.to_string()));
                            vad.reset();
                        }
                    }
                }
            }
            recv(diagnostics) -> _ => {
                tracing::debug!(raw_buffers, raw_samples, processed_frames, "audio pipeline diagnostics");
                raw_buffers = 0;
                raw_samples = 0;
                processed_frames = 0;
            }
        }
    }
    let _ = event_sender.send(CaptureEvent::Stopped);
}
