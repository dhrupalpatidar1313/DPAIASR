# ASR Production Plan — whisper.cpp / whisper-rs

**Scope (confirmed this round):** embedded in the existing Rust dictation app, single audio stream, single local user, Windows + Ubuntu/X11, no REST API, no multi-stream service, no cloud/on-prem deployment question — this is a packaging and integration exercise, not a service design exercise. Sections below are written against that scope; anywhere a normal "production ASR" checklist item doesn't apply at this scope, I've said so explicitly rather than padding the plan with irrelevant enterprise concerns.
**Decoding strategy (confirmed):** greedy, matching the <1.5s per-utterance latency target established earlier.

---

## 1. Model Selection — Decision & Justification

| Model | Size | SHA | Verdict |
|---|---|---|---|
| `small` | 466 MiB | `55356645c2b361a969dfd0ef2c5a50d530afd8d5` | Not used — multilingual capacity wasted given English-only lock |
| `small-q5_1` | 181 MiB | `6fe57ddcfdd1c6b07cdcc73aaf620810ce5fc771` | Not used — same reasoning |
| `small-q8_0` | 252 MiB | `bcad8a2083f4e53d648d586b7dbc0cd673d8afad` | Not used — same reasoning |
| **`small.en-q5_1`** | **181 MiB** | **`20f54878d608f94e4a8ee3ae56016571d47cba34`** | **Primary — production default** |
| `small.en-q8_0` | 252 MiB | `9d75ff4ccfa0a8217870d7405cf8cef0a5579852` | Fallback if empirical WER at q5_1 is unacceptable (§4) |
| `small.en` (F32) | 466 MiB | `db8a495a91d927739e50b3fc1cc4c6b8f6c2d022` | One-time benchmarking reference only, not shipped |
| `small.en-tdrz` | 465 MiB | `b6c6e7e89af1a35c08e6de56b66ca6a02a2fdfa1` | Not used — diarization is irrelevant to single-speaker dictation, and it's larger than the plain `.en` model for no benefit here |

**Justification:**
- **`.en` over multilingual `small`:** with language locked to English, the English-only checkpoint is the right choice — it doesn't split model capacity across ~99 languages, and Whisper's own `.en` variants match or slightly beat the multilingual equivalent on English WER at this size, on top of skipping any language-ID ambiguity entirely.
- **No K-quant exists for `small` in this repo** — your file listing confirms only `q5_1`/`q8_0` are published (this is exactly the gap I flagged last round: the "Q4_K_M" framing in your original research doesn't map onto this repo's actual small-tier offerings).
- **`q5_1` as the default, not `q8_0`:** 181 MiB vs. 252 MiB (~72% the size), faster to load and lower resident memory, with typically a small WER delta between whisper.cpp's own `q5_1`/`q8_0` quantizations — "typically" is doing real work in that sentence, which is why §4 makes verifying this on your own voice a required step, not an optional one.
- **SHA values matter beyond documentation** — use them for download integrity verification, not just as a reference table (§6).

---

## 2. whisper-rs Parameters — Values & Reasoning

Mapped from your CLI-flag question onto the actual `whisper-rs` / `whisper_full_params` fields (verified against current whisper-rs 0.16):

| Parameter | Value | Reasoning |
|---|---|---|
| `n_threads` | 4 (of 6) | Leaves headroom for the audio/VAD thread and OS/UI; **verify empirically** — smaller models often don't scale linearly past 4 threads, don't assume 4 is optimal without measuring 2/4/6 (§4) |
| `n_processors` (CLI-level chunk parallelism) | 1 / not used | This flag parallelizes decoding *across chunks of one long audio file* — irrelevant here, since VAD already hands you short, independent utterances rather than one long file to split |
| `strategy` | `SamplingStrategy::Greedy { n_past: 0 }` | Confirmed this round; matches the mainline whisper-rs API (not the `best_of`-field fork variant some forks expose) |
| `--beam-size` | N/A | Only applies to `BeamSearch` strategy — not used given the greedy decision. Keep as a documented future lever if the latency budget is ever relaxed |
| `--best-of` | N/A at temperature 0 | `best_of` matters for multi-sample decoding at non-zero temperature; pure greedy doesn't use it. If you want a quality safety net, that's what `temperature_inc` (the fallback ladder) is for, not `best_of` |
| `language` | `Some("en")` | Locked; also skips language-ID entirely |
| `translate` | `false` | You want transcription, not translation-into-English — irrelevant here since input is already English |
| `no_context` | `true` | Utterances are independent (VAD-segmented) — don't let decoder context leak from a prior, unrelated utterance |
| `single_segment` | `true` | Recommended addition: since VAD has already guaranteed "this buffer is one coherent utterance," forcing single-segment output avoids Whisper internally fragmenting it at an awkward point |
| `suppress_nst` (suppress non-speech tokens) | `true` | Reduces hallucinated filler on marginal audio |
| `no_speech_thold` | Keep near default (~0.6), don't disable | VAD is your primary speech/non-speech gate; this is defense-in-depth against anything that slipped past VAD (a loud click with energy but no speech) |
| `word_thold` | Default | Only matters if you consume word-level timestamps, which you don't yet — revisit if you add word-level highlighting later |
| `entropy_thold` / `logprob_thold` | Library defaults initially | These drive the temperature-fallback retry decision; tuning blind risks either too many retries (latency) or too few (bad transcripts kept) — tune only after §4's benchmarking shows a real problem, not preemptively |
| `temperature` / `temperature_inc` | Default fallback ladder enabled | Cheap safety net — only triggers on poor-confidence output, not most utterances. **Monitor retry frequency in benchmarking** — if it fires often on your actual voice/mic, it will regularly blow the 1.5s budget and needs attention |
| `print_special` / `print_progress` / `print_realtime` / `print_timestamps` | all `false` | Noise; also see §5's logging-trampoline point — this doesn't mean "ignore whisper.cpp's diagnostics," it means "don't let them go to an invisible stderr" |
| `audio_ctx` (advanced) | Default for now, documented tuning lever | Can trim encoder context for short inputs to save encode time — worth revisiting in §4 if short utterances aren't hitting budget even after thread/quant tuning |

---

## 3. System Architecture & Integration Design

Unchanged from the plan established last round, restated here as the authoritative version for this document:

```text
trait Asr {
    fn transcribe(&mut self, pcm: &[f32]) -> Result<Transcript, AsrError>;
}

struct WhisperAsr { ctx: WhisperContext }   // WhisperState created once, reused
struct NullAsr;                             // test double, no model file required

struct Transcript {
    text: String,
    segments: Vec<Segment>,   // captured now even if unused yet — cheap from state.as_iter()
}
```

- **One dedicated ASR worker thread**, model loaded once at startup, `WhisperState` created once and reused across every utterance for the app's lifetime — never recreate per call.
- **Actor pattern**, not shared state: the worker thread owns `WhisperContext`/`WhisperState` exclusively; only `Vec<f32>` buffers (in) and `Result<Transcript, AsrError>` (out) cross the channel boundary. This sidesteps any `Send`/`Sync` questions about the underlying C++ pointers entirely, rather than fighting them.
- **`NullAsr`** keeps unit tests (channel wiring, backpressure, error propagation) independent of a 181 MiB model file on disk — same rationale as your existing `EnergyVad` test double.

```text
VAD utterance event ──► bounded channel ──► ASR worker (WhisperAsr, single instance)
                                                     │
                                          Result<Transcript, AsrError>
                                                     │
                                        (this phase: log/benchmark it;
                                         target resolution/injection is a later phase)
```

---

## 4. Performance Benchmarking & Tuning Strategy

**Don't trust vendor WER/speed claims for this exact setup — measure them.** Extend your existing `test_audio_wav.rs` harness (already exercises capture → VAD → utterance buffers) one stage further:

**Metrics to capture per utterance:**
- Model load time (once, at startup — not per-utterance, but worth logging once)
- Inference wall-clock time
- Real-time factor (`RTF = inference_time / utterance_duration`) — this is the number that tells you if you're within budget, not raw inference time alone
- Whether the temperature-fallback ladder fired (retry count) — a proxy for how often the model is uncertain on your voice/mic/environment

**Benchmark matrix — run this before finalizing anything:**

| Axis | Values to test |
|---|---|
| Quantization | `small.en-q5_1` vs `small.en-q8_0` (and optionally F32 `small.en` as an accuracy ceiling) |
| Utterance length bucket | short (<1s), medium (1–3s), long (3s+, up to your `MAX_UTTERANCE_DURATION` cap) |
| Thread count | 2, 4, 6 — confirm 4 is actually the sweet spot rather than assumed |

**Tuning levers, in priority order, if the <1.5s target isn't met:**
1. Confirm thread count empirically (small models frequently don't scale past 4 threads — more threads can even be *slower* past a point due to overhead)
2. Drop from `q8_0` to `q5_1` if you'd started with the larger one
3. Trim `audio_ctx` for short utterances (advanced lever from §2)
4. Last resort: fall back to `base.en` (smaller, faster, measurably worse WER) — only if the above don't close the gap, and only after confirming via this same benchmark harness that `small.en-q5_1` genuinely can't hit budget on your hardware, not as a preemptive hedge

This is also where you settle the `q5_1` vs `q8_0` question from §1 with your own data instead of an unverifiable claim: run the same fixed WAV fixture through both, diff the transcripts against what you know was actually said, and decide from there.

---

## 5. Error Handling, Monitoring, Logging

**Scope correction up front:** "monitoring" at this scope means local diagnostic logging you can read yourself, not a metrics/alerting service — dashboards, aggregation, and alerting are the wrong tool for a single-user embedded app and would be pure overhead here.

- **Startup failure must be visible, not silent.** Given the app launches with no console (FR1), a missing/corrupt/incompatible model file must produce a clear, visible failure — not a process that just quietly does nothing. Tie this to a distinct exit code and a log line surfaced via whatever the tray icon can show.
- **Install whisper.cpp's logging trampoline** (`install_whisper_log_trampoline` or the `tracing` variant, both present in current whisper-rs) and route it into your existing `logging/` module. Without this, whisper.cpp's own internal errors go to an unattached `stderr` and vanish — exactly the failure mode a mismatched model file (§1's earlier `transcribe.cpp`-vs-`whisper.cpp` risk) would trigger.
- **Per-utterance error isolation:** a single utterance failing to transcribe (a whisper.cpp internal error, not a model-load failure) must not crash the worker thread or take down subsequent utterances in the session. Catch it, emit an `AsrError` result for that one utterance (which feeds into the pending-queue/error-flash indicator behavior from the broader architecture), and keep processing the next queued item normally.
- **What to log (metadata only, per your established privacy stance):** per-utterance latency, RTF, retry count, queue depth — never the transcript content itself in persistent logs, consistent with the no-persistent-transcript decision made earlier.

---

## 6. Scalability & Deployment (reframed for confirmed scope)

**"Scalability"** here doesn't mean horizontal service scaling — it means: can the pipeline stay correct under your own real usage patterns (talking continuously without pauses, long dictation sessions)? This is already covered by the ASR queue's FIFO ordering and the `Busy`-state backpressure mechanism from the core architecture — nothing new needed here, just confirming it still applies once ASR is a real component instead of a placeholder.

**"Deployment"** means packaging a single binary per OS, and specifically: **don't bundle the 181–252 MiB model file inside the compiled executable.** Recommended pattern:
1. On first run (or via a setup step), download the chosen `.gguf`/`.bin` file directly from `ggerganov/whisper.cpp`.
2. **Verify against the SHA in the table in §1** before ever loading it into `WhisperContext`. This is the concrete fix for the `transcribe.cpp`-vs-`whisper.cpp` compatibility risk flagged earlier — a hash mismatch fails loudly and immediately, rather than whisper-rs failing mysteriously (or worse, loading something subtly wrong) later.
3. Cache it in a local app-data directory (`%APPDATA%` on Windows, `~/.local/share` or XDG equivalent on Linux), not next to the executable, so reinstalling the app doesn't require re-downloading the model.

Since this stays a personal tool rather than something distributed to other end users (per confirmed scope), code-signing/installer polish stays lower priority than it would be for wider distribution — but the SHA-verification step is worth doing regardless, since it protects against a corrupted download even when you're the only user.

---

## 7. Executive Summary

Model: `small.en-q5_1` (181 MiB), verified against its published SHA before load, with `small.en-q8_0` as a fallback pending your own empirical WER comparison — not the `Q4_K_M`/handy-computer path from the original research, which doesn't exist for this model size in the canonical repo. Decoding stays greedy per the confirmed latency priority, with `no_context`/`single_segment`/`suppress_nst` set to match the fact that each utterance is an independent, VAD-pre-segmented unit rather than a continuous stream. The architecture is unchanged from what was already planned — one worker thread, one long-lived model instance, actor-pattern channel boundary — this document's real contribution is pinning down the specific parameter values, the model file's integrity verification, and making sure a silent background app fails loudly instead of silently when the model doesn't load correctly.
