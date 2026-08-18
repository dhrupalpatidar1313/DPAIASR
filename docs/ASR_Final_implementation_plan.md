# Technical Implementation Plan — DPAIASR ASR Subsystem

## Confirmed Decisions (Pre-Recorded)

| # | Decision | Chosen |
|---|---|---|
| A | GPU phase scope | CPU baseline in full detail; CUDA described at high level / stubbed for later |
| B | Model download location | Dedicated `src/asr/model_manager.rs` invoked at startup |
| C | HTTP client | `reqwest` + `tokio` (streaming, progress reporting) |
| D | Channel type | `crossbeam-channel` (already in project) |
| E | Logging trampoline | `install_whisper_log_trampoline` mapped to `tracing` macros |
| F | `auto` on CPU-only build | Log a `debug!` line "GPU backend not compiled", select CPU |

---

## 1. Crates / Libraries

### Production (added to `[dependencies]` in `Cargo.toml`)

| Crate | Version | Why |
|---|---|---|
| `whisper-rs` | `0.16` | Safe Rust bindings for whisper.cpp; provides `WhisperContext`, `WhisperState`, `FullParams`, log trampoline |
| `reqwest` | `0.12` with `features = ["stream", "rustls-tls"]` | Async HTTP client for streaming model download (~181 MiB); rustls avoids an OpenSSL system dependency |
| `tokio` | `1` with `features = ["rt-multi-thread", "macros", "fs", "io-util"]` | Async runtime required by `reqwest`; also used for `tokio::fs` for async file I/O in model caching |
| `sha2` | `0.10` | SHA-256 digest computation for model integrity verification |
| `hex` | `0.4` | Encode `sha2` digest bytes to a hex string for comparison against the published SHA |
| `futures-util` | `0.3` | `StreamExt` for consuming `reqwest`'s `bytes_stream()` in the download loop |
| `dirs` | `5` | Cross-platform `dirs::data_local_dir()` — `%APPDATA%` on Windows, `~/.local/share` on Linux |

> **Note on `whisper-rs` 0.16:** the plan's `whisper_full_params` field names (e.g., `no_context`, `single_segment`, `suppress_nst`) have been verified against whisper-rs 0.16's public API. Double-check after `cargo add whisper-rs@0.16` that those field names compile.

### Development only (added to `[dev-dependencies]`)

| Crate | Version | Why |
|---|---|---|
| `hound` | `3.5` | Already present; used in WAV harness extension |
| `tempfile` | `3` | Create isolated temp dirs in model-manager unit tests without touching `%APPDATA%` |

### Feature-gated (CPU-only build compiles without these)

| Feature flag | Notes |
|---|---|
| `cuda` | Passes CUDA CMake defines through `whisper-rs`'s `build.rs`; no separate crate needed |

---

## 2. `Cargo.toml` Changes

### `[features]` section (NEW)

```toml
[features]
default = []
cuda = []       # enables whisper.cpp CUDA backend; CPU build works without this
vulkan = []     # reserved for future Phase 2 GPU, not implemented yet
```

### `[dependencies]` additions

```toml
whisper-rs    = "0.16"
reqwest       = { version = "0.12", features = ["stream", "rustls-tls"] }
tokio         = { version = "1",    features = ["rt-multi-thread", "macros", "fs", "io-util"] }
sha2          = "0.10"
hex           = "0.4"
futures-util  = "0.3"
dirs          = "5"
```

### `[dev-dependencies]` additions

```toml
tempfile = "3"
```

---

## 3. Overall Architecture

```text
Mic
 ↓
AudioCapture
 ↓
AudioPreprocessor
 ↓
SileroVad
 ↓
VadEvent::Utterance
 ↓
AsrWorkerHandle.enqueue(Vec<f32>)
 ↓
[bounded crossbeam-channel]
 ↓
AsrWorker thread
 ↓
AsrEngine::transcribe()
 ├── CpuWhisperAsr  (default)
 └── GpuWhisperAsr  (stub, --features cuda)
 ↓
Result<Transcript, AsrError>
 ↓
transcript_rx channel
 ↓
main loop: log RTF + text (Phase 1)
           inject (later phase)

startup sequence:
  ModelManager::ensure_model() → path
  BackendResolver::resolve()   → BackendResolution
  CpuWhisperAsr::new()         → engine
  AsrWorker::spawn(engine)     → AsrWorkerHandle
```

### Channel Boundary Rule
Only `Vec<f32>` (PCM audio) crosses **into** the worker.
Only `Result<Transcript, AsrError>` crosses **out**.
`WhisperContext` and `WhisperState` never leave the worker thread.

---

## 4. Files to Create

### 4.1 `src/asr/mod.rs` (NEW)
Public module root; re-exports everything the rest of the app needs.

```rust
pub mod backend;
pub mod cpu;
pub mod engine;
pub mod model_manager;
pub mod resolver;
pub mod worker;

#[cfg(feature = "cuda")]
pub mod gpu;

pub use backend::{AsrBackendKind, BackendPreference};
pub use engine::{AsrEngine, AsrError, NullAsr, Segment, Transcript};
pub use model_manager::{ModelManager, ModelManagerConfig, ModelManagerError};
pub use resolver::{BackendResolution, BackendResolver, BackendResolverConfig};
pub use worker::{AsrWorker, AsrWorkerConfig, AsrWorkerHandle};
```

---

### 4.2 `src/asr/engine.rs` (NEW)
The `AsrEngine` trait, `Transcript`, `Segment`, `AsrError`, and `NullAsr` test double.

**Types:**

```rust
#[derive(Debug, Clone)]
pub struct Segment {
    pub start_ms: i64,
    pub end_ms:   i64,
    pub text:     String,
}

#[derive(Debug, Clone)]
pub struct Transcript {
    pub text:     String,
    pub segments: Vec<Segment>,
}

#[derive(Debug, thiserror::Error)]
pub enum AsrError {
    #[error("model not loaded: {0}")]
    ModelNotLoaded(String),
    #[error("transcription failed: {0}")]
    Inference(String),
    #[error("empty audio buffer passed to ASR")]
    EmptyBuffer,
    #[error("worker thread panicked")]
    WorkerPanicked,
    #[error("ASR queue is full; utterance dropped")]
    QueueFull,
}

pub trait AsrEngine: Send + 'static {
    fn transcribe(&mut self, pcm: &[f32]) -> Result<Transcript, AsrError>;
}

pub struct NullAsr;
impl AsrEngine for NullAsr {
    fn transcribe(&mut self, _pcm: &[f32]) -> Result<Transcript, AsrError> {
        Ok(Transcript { text: String::new(), segments: vec![] })
    }
}
```

---

### 4.3 `src/asr/backend.rs` (NEW)
`AsrBackendKind` enum and `BackendPreference` for runtime config parsing.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsrBackendKind {
    Cpu,
    #[cfg(feature = "cuda")]
    Cuda,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendPreference { Cpu, Gpu, Auto }

impl std::str::FromStr for BackendPreference {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "cpu"  => Ok(Self::Cpu),
            "gpu"  => Ok(Self::Gpu),
            "auto" => Ok(Self::Auto),
            other  => Err(format!("unknown backend preference '{other}'"))
        }
    }
}
```

---

### 4.4 `src/asr/resolver.rs` (NEW)
`BackendResolver` — separates three decisions: hardware available? backend compiled? selected.

**Structs:**

```rust
pub struct BackendResolverConfig {
    pub preference:  BackendPreference,
    pub cpu_threads: u32,
    pub gpu_device:  u32,
}

pub struct BackendResolution {
    pub backend_kind: AsrBackendKind,
    pub cpu_threads:  u32,
    pub gpu_device:   Option<u32>,
    pub reason:       String,    // logged at info level at startup
}
```

**`BackendResolver::resolve()` logic:**

```
match preference:
  Cpu  → BackendResolution { Cpu, reason: "user requested CPU" }

  Gpu  →
    #[cfg(feature = "cuda")]     → BackendResolution { Cuda, reason: "user requested GPU, CUDA compiled" }
    #[cfg(not(feature = "cuda"))] → return Err("backend=gpu but no GPU backend compiled; rebuild with --features cuda")

  Auto →
    #[cfg(feature = "cuda")]     → try CUDA probe; Ok → Cuda; Err → Cpu with warning
    #[cfg(not(...))]             → tracing::debug!("GPU backend not compiled, auto selects CPU");
                                   BackendResolution { Cpu, reason: "auto: no GPU backend compiled" }
```

---

### 4.5 `src/asr/model_manager.rs` (NEW)
Download, SHA-verify, and cache the model. Called once at startup inside `tokio::Runtime::block_on`.

**Config & error types:**

```rust
pub struct ModelManagerConfig {
    pub model_filename:    String,      // "ggml-small.en-q5_1.bin"
    pub expected_sha256:   String,      // "20f54878d608f94e4a8ee3ae56016571d47cba34"
    pub download_base_url: String,      // "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/"
    pub cache_dir:         Option<PathBuf>,
}

impl ModelManagerConfig {
    pub fn default_small_en_q5_1() -> Self { ... }  // pre-filled with the SHA from production plan §1
}

pub enum ModelManagerError {
    NoCacheDir,
    Io(std::io::Error),
    Download(String),
    ShaMismatch { expected: String, actual: String },
}
```

**`ModelManager::ensure_model(config)` algorithm:**

```
1. Resolve cache dir (config.cache_dir or dirs::data_local_dir()/dpaiasr/models)
   tokio::fs::create_dir_all(cache_dir)

2. model_path = cache_dir / model_filename

3. If model_path.exists():
     sha = compute_sha256(model_path)
     if sha == expected_sha256 → return Ok(model_path)
     else → warn! SHA mismatch, remove and re-download

4. Download:
     url = base_url + filename
     response = reqwest::get(url)
     if !response.status().is_success() → Err(Download)

     tmp_path = model_path.with_extension("tmp")
     hasher = sha2::Sha256::new()
     stream = response.bytes_stream()
     for each chunk: write to tmp file, update hasher
                     (log progress every 10 MiB using content-length header)

5. Verify SHA:
     if hex(hasher.finalize()) != expected → remove tmp, Err(ShaMismatch)

6. tokio::fs::rename(tmp_path → model_path)
   return Ok(model_path)
```

**Private async helper:**
```rust
async fn compute_sha256(path: &Path) -> Result<String, ModelManagerError>
// reads in 64 KiB chunks using tokio::io::AsyncReadExt
```

---

### 4.6 `src/asr/cpu.rs` (NEW)
`CpuWhisperAsr` — the concrete CPU backend. Owns one `WhisperContext` and one `WhisperState` for the app's lifetime.

**Struct:**
```rust
pub struct CpuWhisperAsr {
    ctx:       whisper_rs::WhisperContext,
    state:     whisper_rs::WhisperState,
    n_threads: i32,
}
```

**`CpuWhisperAsr::new(model_path, n_threads)` algorithm:**

```
1. Call install_whisper_log_trampoline() exactly once via std::sync::Once.
   Map whisper log levels:
     ERROR → tracing::error!
     WARN  → tracing::warn!
     INFO  → tracing::debug!   (intentionally debug; whisper INFO is noisy)
     other → tracing::trace!

2. ctx_params = WhisperContextParameters::default()
   ctx_params.use_gpu = false

3. ctx = WhisperContext::new_with_params(model_path, ctx_params)?
4. state = ctx.create_state()?
5. return Ok(CpuWhisperAsr { ctx, state, n_threads })
```

**`AsrEngine::transcribe()` algorithm:**

```
1. if pcm.is_empty() → return Err(AsrError::EmptyBuffer)

2. Build FullParams once per call (cheap, no caching needed):
   params = FullParams::new(SamplingStrategy::Greedy { n_past: 0 })
   set_n_threads(n_threads)
   set_language(Some("en"))
   set_translate(false)
   set_no_context(true)
   set_single_segment(true)
   set_suppress_non_speech_tokens(true)
   set_print_special(false)
   set_print_progress(false)
   set_print_realtime(false)
   set_print_timestamps(false)
   // temperature_inc, entropy_thold, logprob_thold, no_speech_thold → defaults

3. self.state.full(params, pcm)?

4. n = self.state.full_n_segments()
   for i in 0..n:
     text   = state.full_get_segment_text(i)?
     t0     = state.full_get_segment_t0(i)?  // centiseconds
     t1     = state.full_get_segment_t1(i)?
     push Segment { start_ms: t0*10, end_ms: t1*10, text }

5. return Ok(Transcript { text: segments joined & trimmed, segments })
```

---

### 4.7 `src/asr/gpu.rs` (NEW — feature-gated stub)

```rust
#![cfg(feature = "cuda")]

pub struct GpuWhisperAsr;

impl GpuWhisperAsr {
    pub fn new(_model_path: &Path, _gpu_device: u32, _n_threads: u32) -> Result<Self, AsrError> {
        Err(AsrError::ModelNotLoaded(
            "GPU backend not yet implemented".into()
        ))
    }
}

impl AsrEngine for GpuWhisperAsr {
    fn transcribe(&mut self, _pcm: &[f32]) -> Result<Transcript, AsrError> {
        Err(AsrError::ModelNotLoaded("GPU backend not yet implemented".into()))
    }
}
```

---

### 4.8 `src/asr/worker.rs` (NEW)
Actor thread that owns the engine. Only `Vec<f32>` and `Result<Transcript, AsrError>` cross the boundary.

**Types:**

```rust
pub struct AsrWorkerConfig {
    pub queue_capacity: usize,  // default: 8
}

pub(crate) struct AsrRequest {
    pub audio: Vec<f32>,
    pub reply: Option<crossbeam_channel::Sender<Result<Transcript, AsrError>>>,
}

pub struct AsrWorkerHandle {
    sender: crossbeam_channel::Sender<AsrRequest>,
}

impl AsrWorkerHandle {
    pub fn enqueue(&self, audio: Vec<f32>) -> Result<(), AsrError> {
        self.sender.try_send(AsrRequest { audio, reply: None })
            .map_err(|_| AsrError::QueueFull)
    }
}
```

**`AsrWorker::spawn()` function:**
```rust
pub fn spawn<E: AsrEngine>(
    engine: E,
    config: AsrWorkerConfig,
    output: crossbeam_channel::Sender<Result<Transcript, AsrError>>,
) -> AsrWorkerHandle {
    let (sender, receiver) = crossbeam_channel::bounded(config.queue_capacity);
    std::thread::Builder::new()
        .name("dpaiasr-asr-worker".into())
        .spawn(move || run_asr_worker(engine, receiver, output))
        .expect("failed to spawn ASR worker thread");
    AsrWorkerHandle { sender }
}
```

**`run_asr_worker()` loop:**

```
for request in receiver:
    started = Instant::now()
    result  = engine.transcribe(&request.audio)
    elapsed = started.elapsed()

    match &result:
        Ok(t)  → info! inference_ms, utterance_ms, rtf, segments (NO transcript text in logs)
        Err(e) → error! error

    if let Some(reply) = request.reply → send to reply channel (test mode)
    if output.send(result).is_err() → warn! and break
```

---

### 4.9 `tests/asr_cpu_integration.rs` (NEW)

Four tests:

1. `null_asr_accepts_any_pcm` — `NullAsr`, no model file required.
2. `cpu_whisper_transcribes_test_audio_wav` — Real model + `test_audio.wav`; skips if model missing; asserts `rtf < 3.0`.
3. `cpu_whisper_rejects_empty_pcm` — Expects `Err(AsrError::EmptyBuffer)`.
4. `asr_worker_queues_and_returns_transcripts` — `NullAsr` + worker; 5 frames in, 5 `Ok` out; FIFO.

---

### 4.10 `tests/model_manager_unit.rs` (NEW)

Three tests:

1. `cache_hit_skips_download` — Place file with correct SHA in `tempdir`; verify no download.
2. `sha_mismatch_triggers_redownload` — Wrong content in cache; expect `ShaMismatch` or download error.
3. `missing_model_returns_download_error` — Empty tempdir + unreachable URL; expect `Download` error.

---

## 5. Files to Modify

### 5.1 `Cargo.toml`
Add `[features]`, production dependencies, and `tempfile` to `[dev-dependencies]`.

### 5.2 `src/lib.rs`
Add:
```rust
pub mod asr;

pub use asr::{AsrEngine, AsrError, NullAsr, Transcript, Segment};
pub use asr::{AsrWorkerHandle, AsrWorkerConfig, AsrWorker};
pub use asr::{ModelManagerConfig, ModelManagerError};
pub use asr::{BackendResolverConfig, BackendResolution};
```

### 5.3 `src/main.rs`
Changes in order of execution:

1. Create `tokio::Runtime`, call `ModelManager::ensure_model()` inside `block_on`.
2. Call `BackendResolver::resolve()`, log result at `info!`.
3. Instantiate `CpuWhisperAsr::new(model_path, n_threads)`.
4. Create `(transcript_tx, transcript_rx)` bounded channel.
5. Call `AsrWorker::spawn(engine, config, transcript_tx)` → `asr_handle`.
6. In the VAD event loop, add arm:
   ```rust
   CaptureEvent::Vad(VadEvent::Utterance { audio, .. }) => {
       if let Err(e) = asr_handle.enqueue(audio) {
           tracing::warn!(error = %e, "ASR queue full; utterance dropped");
       }
   }
   ```
7. In the same loop iteration, drain `transcript_rx` with `try_recv()`:
   ```rust
   while let Ok(result) = transcript_rx.try_recv() {
       match result {
           Ok(t)  => tracing::info!(text = %t.text, "transcript"),
           Err(e) => tracing::error!(error = %e, "ASR error"),
       }
   }
   ```

### 5.4 `tests/test_audio_wav.rs`
Add one new test `test_audio_file_utterances_reach_asr_queue` using `NullAsr` + `AsrWorkerHandle` to verify VAD→ASR queue end-to-end without the real model file.

---

## 6. Module and Type Index

| Symbol | File | Kind | Notes |
|---|---|---|---|
| `AsrEngine` | `engine.rs` | `trait` | Core transcription interface |
| `AsrError` | `engine.rs` | `enum` | All ASR errors |
| `Transcript` | `engine.rs` | `struct` | Transcription output |
| `Segment` | `engine.rs` | `struct` | One whisper segment |
| `NullAsr` | `engine.rs` | `struct` | Test double |
| `AsrBackendKind` | `backend.rs` | `enum` | Cpu / Cuda (feature-gated) |
| `BackendPreference` | `backend.rs` | `enum` | cpu / gpu / auto |
| `BackendResolverConfig` | `resolver.rs` | `struct` | Input to resolver |
| `BackendResolution` | `resolver.rs` | `struct` | Output of resolver |
| `BackendResolver` | `resolver.rs` | `struct` | Stateless resolver (one method) |
| `ModelManagerConfig` | `model_manager.rs` | `struct` | Filename, SHA, URL, cache dir |
| `ModelManagerError` | `model_manager.rs` | `enum` | Download / SHA / IO errors |
| `ModelManager` | `model_manager.rs` | `struct` | `ensure_model()` entry point |
| `compute_sha256` | `model_manager.rs` | `async fn` | Private — 64 KiB chunked hash |
| `CpuWhisperAsr` | `cpu.rs` | `struct` | CPU backend (implements `AsrEngine`) |
| `GpuWhisperAsr` | `gpu.rs` | `struct` | CUDA stub (feature-gated) |
| `AsrWorkerConfig` | `worker.rs` | `struct` | Queue capacity |
| `AsrRequest` | `worker.rs` | `struct` | Per-utterance work item (pub(crate)) |
| `AsrWorkerHandle` | `worker.rs` | `struct` | Cloneable enqueue handle |
| `AsrWorker` | `worker.rs` | `struct` | Namespace for `spawn()` |
| `run_asr_worker` | `worker.rs` | `fn` | Worker body (private) |

---

## 7. Architecture Trade-offs

| Decision | Trade-off | Justification |
|---|---|---|
| `CpuWhisperAsr` owns `WhisperState` directly | Cannot share across threads | Avoids `Send`/`Sync` fights with raw C++ pointers; actor pattern makes sharing unnecessary |
| `FullParams` rebuilt each `transcribe()` call | Tiny allocation per utterance | `FullParams::new()` is cheap; caching requires fighting whisper-rs lifetime annotations |
| SHA-256 verified on cache hit too | ~200ms overhead on startup | Detects corruption of the cached file; acceptable once-per-startup cost |
| `reqwest` + `tokio` for model download | ~3 MiB of async runtime | Progress reporting on 181 MiB downloads is a usability requirement; `ureq` cannot stream progress |
| `install_whisper_log_trampoline` behind `std::sync::Once` | Global side-effect | whisper-rs documents this as a global; calling twice is undefined |
| CUDA backend is a stub returning `Err` | No GPU acceleration yet | Interface fully defined; Phase 2 adds only `gpu.rs` implementation without touching any other file |
| RTF logged via `tracing`, not exported | No metrics sink | Single-user embedded app; `RUST_LOG=info cargo run` gives per-utterance RTF in the terminal |
| `temperature_inc`/`entropy_thold` at defaults | May trigger retries | Plan §2 explicitly says: tune only after benchmarking shows a real problem |

---

## 8. Startup Exit Codes

| Failure | Exit code | How surfaced |
|---|---|---|
| `ModelManagerError::ShaMismatch` | `2` | `tracing::error!` + `eprintln!` |
| `ModelManagerError::Download(..)` | `3` | Same |
| `AsrError::ModelNotLoaded(..)` | `4` | Same |
| Any other `anyhow::Error` from `main()` | `1` | `anyhow` default display |

---

## 9. Implementation Order (Sequential)

```
Step 1:  Update Cargo.toml → cargo check
Step 2:  src/asr/engine.rs → test NullAsr + Send
Step 3:  src/asr/backend.rs → test FromStr
Step 4:  src/asr/resolver.rs → test Auto on CPU-only build
Step 5:  src/asr/model_manager.rs → tests/model_manager_unit.rs
Step 6:  src/asr/cpu.rs → test empty buffer + 1s silence
Step 7:  src/asr/gpu.rs (stub, feature-gated)
Step 8:  src/asr/worker.rs → test FIFO ordering with NullAsr
Step 9:  src/asr/mod.rs → cargo check --all-targets
Step 10: Modify src/lib.rs
Step 11: Modify src/main.rs → cargo run smoke test
Step 12: Extend tests/test_audio_wav.rs
Step 13: Create tests/asr_cpu_integration.rs
Step 14: cargo test --all-targets
Step 15: cargo clippy --all-targets --all-features -- -D warnings
Step 16: cargo fmt --check
```

---

## 10. What Is Explicitly Out of Scope

| Item | Reason |
|---|---|
| Transcript injection into focused app | Later phase |
| Vulkan GPU backend | Phase 2 of GPU; feature flag declared only |
| Word-level timestamps | `word_thold` noted as a future lever |
| `audio_ctx` tuning | Tune only if §4 benchmarks show it's needed |
| Monitoring dashboards | Out of scope for single-user embedded app (plan §5) |
| Code signing / installer | Lower priority for personal tool (plan §6) |
