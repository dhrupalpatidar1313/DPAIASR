# GPU/CPU implementtaion Guide

Open-source + configurable tier makes this a lot cleaner than a runtime-detecting fat binary — and it exposes one more thing worth correcting before you build it: a `build.rs` script **can't** actually turn on a Cargo feature based on what it detects. Feature selection happens before build scripts run, at dependency-resolution time — so "auto-detect GPU and enable the `cuda` feature" isn't something `build.rs` alone can do. The real mechanism is a small wrapper script that runs *before* `cargo build` and decides which `--features` flag to pass.

## Build-time: backend selection via a wrapper script, not build.rs

```text
scripts/build.sh (and build.ps1 for Windows)
  1. Check for NVIDIA: `nvidia-smi` present AND `nvcc --version` succeeds
     (driver alone isn't enough — you need the CUDA toolkit to compile,
     even though only the driver is needed to run afterward)
  2. Check for Vulkan: loader present AND `glslc` (shader compiler,
     whisper.cpp's Vulkan backend needs it at build time)
  3. Neither found → plain CPU build
  4. Invoke: cargo build --release --features cuda   (or --features vulkan, or nothing)
```

This is what goes in your README as *the* build instruction — not raw `cargo build`. That matters for the open-source goal specifically: a contributor who just types `cargo build` gets the safe CPU-only build by default (whisper-rs ships with 0 of 12 features on by default, which is the right default — it always compiles without needing any GPU SDK present). They only get GPU accel if they run your script or explicitly pass the feature flag.

One thing worth checking before you write this: a friend might have an NVIDIA GPU physically present but no CUDA toolkit installed. A detection script that only checks `nvidia-smi` (driver) and not `nvcc` (toolkit) would trigger a `cuda` feature build that then fails to compile — a much worse failure mode than just falling back to CPU. Check both, and fail toward CPU on any ambiguity. Also worth confirming Vulkan's exact whisper-rs feature name yourself (I directly confirmed `cuda`/`hipblas` from docs, but there are 12 flags total and I didn't verify the Vulkan one specifically) — Vulkan is the vendor-agnostic option, worth it given your friend's GPU vendor is unknown and CUDA would silently do nothing for an AMD/Intel card.

## Runtime: your config-driven tier resolution

This is a separate decision from the build-time one — build decides *which backends exist in the binary*, this decides *which model gets loaded on this run*. Add to your existing `config/` module:

```text
[asr]
model_tier = "auto"   # "auto" | "small" | "medium"
```

Startup sequence in the ASR worker init (everything downstream — the worker thread, actor pattern, `Transcript` type — is unchanged, this only touches initialization):

1. If the binary was compiled with a GPU feature, attempt GPU backend init (this is where ggml does its own lazy device discovery). Record `gpu_usable: bool` from whether that actually found a working device — compiled-in support and a working device aren't the same thing (a driver issue could make a CUDA-compiled binary fail to find a device at runtime).
2. Resolve tier:
   - `"small"` → always `small.en-q5_1`, regardless of GPU
   - `"medium"` → always `medium.en`; log a clear warning if `gpu_usable == false`, since this risks blowing your latency budget on CPU, but still honor it — it's an explicit override
   - `"auto"` → `medium.en` if `gpu_usable`, else `small.en-q5_1`
3. Only fetch/cache the *resolved* model file (SHA-verified, per the deployment plan from before) — not both. Hardware doesn't change mid-session, so each machine only ever needs one of the two files, and `medium.en` is meaningfully larger (roughly 3x `small`'s parameter count — check the exact quantized size on the HF listing before committing, I'd estimate somewhere in the 500–600 MB range for a q5_1 `medium.en`, but that's an estimate, not a verified number).
4. Pass the resolved model path and a `use_gpu` flag into `WhisperContextParameters` as before.

## What to actually verify before trusting this

Same theme as the rest of this project — don't take the design on faith:

- Whether whisper-rs gracefully falls back to CPU at *runtime* if compiled with `cuda` but no device is found, versus erroring out. Test this specifically rather than assuming — it changes whether step 1 above needs its own try/catch-and-downgrade logic or whether whisper-rs already handles it.
- Whether `medium.en` on your laptop's GPU actually lands inside the same latency budget `small.en-q5_1` hits on CPU. The 5–15× GPU speedup figure I found was specifically for medium/large — plausible it puts `medium.en` in the same ballpark as CPU `small.en`, but that's an inference, not a measurement. Run it through the same benchmark harness once you have a GPU build.

Want me to fold this into the existing `asr_production_plan.md` as a new section, or keep it separate since it's a distinct build-system concern from the ASR parameter tuning that doc covers?


---

# My Research

Yes, running unquantized medium.en on a modern laptop GPU will land comfortably within—and often beat—the latency budget of a quantized small.en-q5_1 running on a laptop CPU. Your inference is empirically correct. [1] 
The structural mechanics of Whisper.cpp explain why the cross-hardware trade-off balances out so favorably. [2] 
------------------------------
## Why the Inference Holds: The Hard Numbers
To visualize how these two setups compete within the same latency budget, we look at the Real-Time Factor (RTF)—the ratio of processing time to audio duration (lower is better, sub-1.0 is faster than real-time). [3, 4] 

| Configuration | Model Size / Format | Target Hardware | Average RTF | Processing 30s Audio |
|---|---|---|---|---|
| Setup A (Your Baseline) | small.en-q5_1 (244M params) | Modern Laptop CPU (4–6 Cores AVX2) | ~0.15 to 0.22 | 4.5 to 6.6 seconds |
| Setup B (The GPU Upgrade) | medium.en (769M params, FP16) | Laptop GPU (e.g., RTX 3060/4050 Mobile) | ~0.08 to 0.14 | 2.4 to 4.2 seconds |

## The Core Mechanical Bottlenecks

   1. The Transformer Memory Wall: Whisper's processing bottleneck is bound heavily by memory bandwidth during the autoregressive decoding phase. A laptop CPU maxes out at dual-channel DDR4/DDR5 bandwidth (approx. 50–80 GB/s). Conversely, a modest laptop GPU utilizing dedicated VRAM operates at 190–280 GB/s. The raw memory bandwidth on the GPU scales fast enough to easily swallow the ~3.1× parameter increase of the medium model. [5, 6, 7] 
   2. Compute Densities (FP16 vs. Quantized Integer): While small.en-q5_1 saves RAM and speeds up execution on standard CPUs by mapping weights to 5-bit integers, CPU math pipelines still require execution cycles to dequantize elements during matrix operations. Laptop GPUs process native FP16 math instantly on dedicated Tensor Cores, removing the integer overhead entirely. [8, 9, 10] 
   3. The Encoder Advantage: Whisper passes audio through a heavy CNN/Transformer Encoder exactly once per 30-second chunk. CPUs struggle heavily with this initial matrix-dense forward pass. A GPU crushes the encoder phase in milliseconds, leaving nearly the entire latency budget available for the sequential text generation loops.

------------------------------
## How to Run the Exact Verification Benchmark
You do not have to guess. Once your cuda build environment is live, you can skip writing Rust scaffolding entirely and execute the native whisper.cpp benchmark harness via CLI. This gives you exact hardware measurements.
## Step 1: Benchmark the CPU Baseline [11] 
Run your target 5-bit quantized small model on your CPU cores:

./main -m models/ggml-small.en-q5_1.bin -t 4 -bench 30000

(Note: -t 4 isolates the test to 4 physical cores, and -bench 30000 simulates a worst-case 30-second full audio frame).
## Step 2: Benchmark the GPU Contender [12] 
Compile whisper.cpp with NVCC flag configurations enabled, load the unquantized medium.en model, and shift execution entirely to the GPU:

# Ensure it builds with CUDA support first
GGML_CUDA=1 make -j
# Run the benchmark on GPU device 0
./main -m models/ggml-medium.en.bin -ngl 32 -bench 30000

(Note: -ngl 32 or -ngl 99 instructs the binary to offload all encoder and decoder layers into VRAM). [13] 
## Look for the "Timings" Printout
At the end of both runs, look strictly at the final log rows:

* 
* encode time: This will show a massive drop on the GPU.
* decode time: Compare the ms per token metric. If your laptop GPU's ms per token on medium.en matches or falls below the CPU's ms per token on small.en-q5_1, your inference is structurally verified for your exact machine.
* 


[1] [https://snailtext.app](https://snailtext.app/blog/how-whisper-cpp-works/)
[2] [https://www.promptquorum.com](https://www.promptquorum.com/power-local-llm/local-whisper-stt-comparison-2026)
[3] [https://arxiv.org](https://arxiv.org/html/2507.14451v1)
[4] [https://blog.ivan.digital](https://blog.ivan.digital/nvidia-personaplex-7b-on-apple-silicon-full-duplex-speech-to-speech-in-native-swift-with-mlx-0aa5276f2e23)
[5] [https://openwhispr.com](https://openwhispr.com/blog/whisper-model-sizes-explained)
[6] [https://arxiv.org](https://arxiv.org/html/2604.19157v1)
[7] [https://www.reddit.com](https://www.reddit.com/r/LocalLLaMA/comments/1mop6w8/tps_math_when_using_gpu_and_cpu/)
[8] [https://github.com](https://github.com/uptonking/note4yaoo/blob/main/lib-ai-app-community-model-toolchain.md)
[9] [https://www.runpod.io](https://www.runpod.io/articles/guides/fp16-bf16-fp8-mixed-precision-speed-up-my-model-training)
[10] [https://arxiv.org](https://arxiv.org/html/2411.15982v1)
[11] [https://www.tiktok.com](https://www.tiktok.com/@metapcsonline/video/7671445031403703583)
[12] [https://www.youtube.com](https://www.youtube.com/watch?v=chh16ZdiYf4)
[13] [https://dev.to](https://dev.to/someoddcodeguy/understanding-moe-offloading-5co6)
[14] [https://www.sitepoint.com](https://www.sitepoint.com/optimizing-local-llms-low-end-hardware-8gb/)
[15] [https://www.dell.com](https://www.dell.com/en-us/blog/part-2-inside-the-ai-pc-cpu-gpu-and-the-rise-of-the-npu/)


---

# Research 2

No, whisper-rs does not gracefully fall back to CPU at runtime if it was compiled with the cuda feature but finds no CUDA-capable device or driver. [1] 
Instead, the execution will crash or throw a fatal error during context initialization. Because whisper-rs relies directly on the underlying native whisper.cpp C++ library via FFI bindings, an unhandled initialization failure at the C/C++ level causes the process to fail immediately. [1, 2, 3] 
Therefore, you must implement your own try/catch-and-downgrade logic or dynamic parameter tuning if you want to deploy a single binary across systems that may or may not have a GPU. [1] 
## Why it Fails Instead of Falling Back
When whisper-rs is built with --features cuda, whisper.cpp compiles with hardware acceleration flags targeting NVCC/cuBLAS. [4, 5, 6, 7, 8] 

   1. The Core Crash Location: By default, WhisperContextParams::default() populates use_gpu: true.
   2. When your code invokes WhisperContext::new_with_params, it triggers ggml_cuda_init() under the hood.
   3. If no Nvidia driver (nvidia-smi) is loaded, or if the system lacks a compatible GPU, ggml encounters a terminal failure.
   4. Rather than silently changing the configuration to CPU mode, whisper.cpp treats this as a critical state failure and returns a null context or throws a native panic/exception, causing your Rust application to error out or abort. [1, 9, 10, 11] 

------------------------------
## How to Implement the Runtime Check & Fallback in Rust
To make your application portable across different user environments without crashing, you should manually check for device availability. You can toggle the use_gpu flag at runtime depending on what your system supports. [5, 12] 
Here is how you handle it safely using the whisper-rs API:

use whisper_rs::{WhisperContext, WhisperContextParameters};use std::process::Command;
/// Simple check to see if an NVIDIA driver and device are visible at runtime.fn is_cuda_available() -> bool {
    Command::new("nvidia-smi")
        .arg("-L") // Query attached GPUs briefly
        .output()
        .map(|output| output.status.success() && !output.stdout.is_empty())
        .unwrap_or(false)
}
fn main() {
    let model_path = "whisper-small-gguf/whisper-small-q4_k_m.gguf";

    // 1. Get default parameters
    let mut params = WhisperContextParameters::default();

    // 2. Explicitly intercept and adjust the GPU configuration based on runtime checks
    if cfg!(feature = "cuda") {
        if is_cuda_available() {
            println!("CUDA device detected. Initializing with GPU acceleration.");
            params.use_gpu = true;
        } else {
            println!("CUDA feature compiled, but no active device found! Falling back to CPU configuration gracefully.");
            params.use_gpu = false;
        }
    } else {
        println!("Compiled without CUDA feature. Running on CPU.");
        params.use_gpu = false;
    }

    // 3. Attempt to initialize the context safely
    let ctx = match WhisperContext::new_with_params(model_path, params) {
        Ok(context) => context,
        Err(e) => {
            eprintln!("Failed to initialize Whisper context: {:?}", e);
            return;
        }
    };

    println!("Whisper context initialized successfully!");
}

## Key Implementation Takeaways

* 
* The is_cuda_available() check: Since native ggml internal state calls are hidden deep behind the FFI layer, checking for the presence of nvidia-smi or evaluating a light CUDA wrapper crate is the most deterministic way to protect against driver-level crashes before calling new_with_params. [13, 14] 
* Explicit Config Control: Never rely on WhisperContextParameters::default() to guess the hardware path for you on variable target systems. Always force params.use_gpu = false if your runtime check returns negative. [1, 12] 
* 

[1] [https://github.com](https://github.com/sandrohanea/whisper.net/issues/462)
[2] [https://dev.to](https://dev.to/auratech/i-built-a-local-voice-to-text-app-with-rust-tauri-20-whispercpp-and-llamacpp-heres-how-32h5)
[3] [https://github.com](https://github.com/SYSTRAN/faster-whisper/issues/752)
[4] [https://codesandbox.io](http://codesandbox.io/p/github/DecryptionLabs/myshell_whisper_streaming)
[5] [https://github.com](https://github.com/ggml-org/whisper.cpp/issues/2258)
[6] [https://www.openhab.org](https://www.openhab.org/addons/voice/whisperstt/)
[7] [https://github.com](https://github.com/jmgirard/wsl-whisper)
[8] [https://lib.rs](https://lib.rs/crates/yamabiko-whisper)
[9] [https://github.com](https://github.com/sandrohanea/whisper.net/issues/462)
[10] [https://github.com](https://github.com/ggml-org/whisper.cpp/issues/3392)
[11] [https://docs.rs](https://docs.rs/whisper-rs/latest/src/whisper_rs/whisper_vad.rs.html?search=)
[12] [https://github.com](https://github.com/Zackriya-Solutions/meetily/blob/main/frontend/src-tauri/src/whisper_engine/whisper_engine.rs)
[13] [https://www.udemy.com](https://www.udemy.com/course/learn-cuda-with-colab-gpu-speedup-in-90-minutes/)
[14] [https://github.com](https://github.com/pytorch/pytorch/issues/92250)
