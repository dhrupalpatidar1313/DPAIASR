use std::collections::VecDeque;
use crossbeam_channel::{Receiver, Sender, TrySendError};

use crate::pipeline::InjectionJob;
use crate::session::{SessionId, UtteranceId};

#[cfg(target_os = "windows")]
use crate::platform::windows::inject::{Injector, InjectError};
#[cfg(target_os = "windows")]
use crate::platform::TargetSnapshot;

#[derive(Debug, Clone)]
pub enum InjectionResult {
    Started(SessionId, UtteranceId),
    Succeeded(SessionId, UtteranceId),
    Pending(SessionId, UtteranceId),
    Orphaned(SessionId, UtteranceId),
    Failed(SessionId, UtteranceId, String),
    Retryable(SessionId, UtteranceId, String),
    CopiedToClipboard(SessionId, UtteranceId, String),
}

pub enum InjectionCommand {
    NewJob(InjectionJob),
    RetryPending,
    FlushOrphaned,
    CopyLatestOrphaned,
}

#[derive(Debug, Clone)]
pub struct InjectionWorkerConfig {
    pub queue_capacity: usize,
    pub max_pending: usize,
}

impl Default for InjectionWorkerConfig {
    fn default() -> Self {
        Self {
            queue_capacity: 16,
            max_pending: 16,
        }
    }
}

#[derive(Clone)]
pub struct InjectionWorkerHandle {
    sender: Sender<InjectionCommand>,
}

impl InjectionWorkerHandle {
    pub fn enqueue(&self, job: InjectionJob) -> Result<(), TrySendError<InjectionCommand>> {
        self.sender.try_send(InjectionCommand::NewJob(job))
    }
    
    pub fn retry_pending(&self) -> Result<(), TrySendError<InjectionCommand>> {
        self.sender.try_send(InjectionCommand::RetryPending)
    }

    pub fn flush_orphaned(&self) -> Result<(), TrySendError<InjectionCommand>> {
        self.sender.try_send(InjectionCommand::FlushOrphaned)
    }

    pub fn copy_orphaned_to_clipboard(&self) -> Result<(), TrySendError<InjectionCommand>> {
        self.sender.try_send(InjectionCommand::CopyLatestOrphaned)
    }
}

pub struct InjectionWorker;

impl InjectionWorker {
    pub fn spawn(
        config: InjectionWorkerConfig,
        output: Sender<InjectionResult>,
    ) -> InjectionWorkerHandle {
        let (sender, receiver) = crossbeam_channel::bounded(config.queue_capacity);

        std::thread::Builder::new()
            .name("dpaiasr-injector".into())
            .spawn(move || run_injection_worker(config, receiver, output))
            .expect("failed to spawn injection worker thread");

        InjectionWorkerHandle { sender }
    }
}

fn run_injection_worker(
    config: InjectionWorkerConfig,
    receiver: Receiver<InjectionCommand>,
    output: Sender<InjectionResult>,
) {
    tracing::info!("Injection worker thread started");

    #[cfg(target_os = "windows")]
    let mut injector = match Injector::new() {
        Ok(inj) => inj,
        Err(e) => {
            tracing::error!("Failed to initialize Windows Injector: {}", e);
            return;
        }
    };

    let mut waiting_queue: VecDeque<InjectionJob> = VecDeque::new();
    let mut orphaned_queue: VecDeque<InjectionJob> = VecDeque::new();

    loop {
        match receiver.recv() {
            Ok(InjectionCommand::NewJob(job)) => {
                #[cfg(target_os = "windows")]
                process_job_windows(&mut injector, job, &output, config.max_pending, &mut waiting_queue, &mut orphaned_queue);
                #[cfg(not(target_os = "windows"))]
                process_job_mock(job, &output);
            }
            Ok(InjectionCommand::RetryPending) => {
                tracing::info!("Manual retry triggered for pending jobs");
                let jobs: Vec<_> = waiting_queue.drain(..).collect();
                for job in jobs {
                    #[cfg(target_os = "windows")]
                    process_job_windows(&mut injector, job, &output, config.max_pending, &mut waiting_queue, &mut orphaned_queue);
                    #[cfg(not(target_os = "windows"))]
                    process_job_mock(job, &output);
                }
            }
            Ok(InjectionCommand::FlushOrphaned) => {
                tracing::info!("Flushing {} orphaned jobs", orphaned_queue.len());
                orphaned_queue.clear();
            }
            Ok(InjectionCommand::CopyLatestOrphaned) => {
                if let Some(job) = orphaned_queue.pop_back() {
                    let text = job.transcript.text.clone();
                    match arboard::Clipboard::new() {
                        Ok(mut clipboard) => {
                            if let Err(e) = clipboard.set_text(text.clone()) {
                                tracing::error!("Failed to copy orphaned transcript to clipboard: {:?}", e);
                                let _ = output.send(InjectionResult::Failed(job.session_id, job.utterance_id, format!("Clipboard copy failed: {}", e)));
                            } else {
                                tracing::info!("Copied orphaned transcript to clipboard: {:?}", text);
                                let _ = output.send(InjectionResult::CopiedToClipboard(job.session_id, job.utterance_id, text));
                            }
                        }
                        Err(e) => {
                            tracing::error!("Failed to open clipboard: {:?}", e);
                            let _ = output.send(InjectionResult::Failed(job.session_id, job.utterance_id, format!("Clipboard init failed: {}", e)));
                        }
                    }
                } else {
                    tracing::warn!("Alt+C pressed but orphaned queue is empty");
                }
            }
            Err(_) => break,
        }
    }

    tracing::info!("Injection worker thread exiting");
}

#[cfg(target_os = "windows")]
fn process_job_windows(
    injector: &mut Injector,
    job: InjectionJob,
    output: &Sender<InjectionResult>,
    max_pending: usize,
    waiting_queue: &mut VecDeque<InjectionJob>,
    orphaned_queue: &mut VecDeque<InjectionJob>,
) {
    let _ = output.send(InjectionResult::Started(job.session_id, job.utterance_id));

    if job.transcript.text.trim().is_empty() {
        tracing::debug!("Empty transcript, ignoring");
        let _ = output.send(InjectionResult::Succeeded(job.session_id, job.utterance_id));
        return;
    }

    if let Some(ref target) = job.target {
        if !target.is_alive() {
            tracing::info!("Target window closed. Moving to orphaned queue.");
            let _ = output.send(InjectionResult::Orphaned(job.session_id, job.utterance_id));
            if orphaned_queue.len() < max_pending {
                orphaned_queue.push_back(job);
            }
            return;
        }

        let current_target = TargetSnapshot::current();
        if current_target != Some(*target) {
            tracing::debug!("Focus mismatch. Keeping in waiting queue.");
            let _ = output.send(InjectionResult::Pending(job.session_id, job.utterance_id));
            if waiting_queue.len() < max_pending {
                waiting_queue.push_back(job);
            } else {
                let _ = output.send(InjectionResult::Failed(job.session_id, job.utterance_id, "Waiting queue full".into()));
            }
            return;
        }

        match injector.inject(&job.transcript.text, target) {
            Ok(()) => {
                let _ = output.send(InjectionResult::Succeeded(job.session_id, job.utterance_id));
            }
            Err(InjectError::FocusLost { .. }) => {
                tracing::warn!("Focus lost during injection.");
                let _ = output.send(InjectionResult::Pending(job.session_id, job.utterance_id));
                if waiting_queue.len() < max_pending {
                    waiting_queue.push_back(job);
                } else {
                    let _ = output.send(InjectionResult::Failed(job.session_id, job.utterance_id, "Waiting queue full".into()));
                }
            }
            Err(InjectError::SystemError(msg)) => {
                tracing::error!("System error during injection: {}", msg);
                let _ = output.send(InjectionResult::Failed(job.session_id, job.utterance_id, msg));
            }
        }
    } else {
        let _ = output.send(InjectionResult::Failed(job.session_id, job.utterance_id, "No target snapshot".into()));
    }
}

#[cfg(not(target_os = "windows"))]
fn process_job_mock(
    job: InjectionJob,
    output: &Sender<InjectionResult>,
) {
    let _ = output.send(InjectionResult::Started(job.session_id, job.utterance_id));
    tracing::info!("Mock injection of: {}", job.transcript.text);
    let _ = output.send(InjectionResult::Succeeded(job.session_id, job.utterance_id));
}
