use crate::asr::engine::{AsrEngine, AsrError};
use crate::pipeline::{AsrJob, InjectionJob};

#[derive(Debug, Clone)]
pub struct AsrWorkerConfig {
    pub queue_capacity: usize,
}

impl Default for AsrWorkerConfig {
    fn default() -> Self {
        Self { queue_capacity: 8 }
    }
}

pub struct AsrWorkerHandle {
    sender: crossbeam_channel::Sender<AsrJob>,
}

impl AsrWorkerHandle {
    pub fn enqueue(&self, job: AsrJob) -> Result<(), AsrError> {
        use crossbeam_channel::TrySendError;
        match self.sender.try_send(job) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(AsrError::QueueFull),
            Err(TrySendError::Disconnected(_)) => Err(AsrError::WorkerPanicked),
        }
    }
}

pub struct AsrWorker;

impl AsrWorker {
    pub fn spawn<E: AsrEngine>(
        engine: E,
        config: AsrWorkerConfig,
        output: crossbeam_channel::Sender<Result<InjectionJob, AsrError>>,
    ) -> AsrWorkerHandle {
        let (sender, receiver) = crossbeam_channel::bounded(config.queue_capacity);

        std::thread::Builder::new()
            .name("dpaiasr-asr-worker".into())
            .spawn(move || run_asr_worker(engine, receiver, output))
            .expect("failed to spawn ASR worker thread");

        AsrWorkerHandle { sender }
    }
}

fn run_asr_worker<E: AsrEngine>(
    mut engine: E,
    receiver: crossbeam_channel::Receiver<AsrJob>,
    output: crossbeam_channel::Sender<Result<InjectionJob, AsrError>>,
) {
    tracing::info!("ASR worker thread started");

    for job in receiver.iter() {
        let started = std::time::Instant::now();
        let result = engine.transcribe(&job.audio);
        let elapsed = started.elapsed();

        let job_result = match result {
            Ok(transcript) => {
                let utterance_duration_secs = job.audio.len() as f32 / 16_000.0;
                let rtf = if utterance_duration_secs > 0.0 {
                    elapsed.as_secs_f32() / utterance_duration_secs
                } else {
                    0.0
                };
                tracing::info!(
                    inference_ms = elapsed.as_millis(),
                    utterance_ms = (utterance_duration_secs * 1000.0) as u64,
                    rtf = format!("{rtf:.3}"),
                    segments = transcript.segments.len(),
                    "ASR inference ok"
                );
                
                Ok(InjectionJob {
                    session_id: job.session_id,
                    utterance_id: job.utterance_id,
                    transcript,
                    target: job.target,
                })
            }
            Err(e) => {
                tracing::error!(error = %e, "ASR inference failed for this utterance");
                Err(e)
            }
        };

        if output.send(job_result).is_err() {
            tracing::warn!("ASR output channel closed; worker exiting");
            break;
        }
    }

    tracing::info!("ASR worker thread exiting");
}
