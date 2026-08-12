# Local Voice Dictation Overlay — System Architecture & Requirements Specification

**Target platform:** Windows 10 x64
**Language/runtime:** Rust (native Win32, single .exe, no installer dependency)
**Hardware baseline (from your Task Manager capture):** Intel i5-9400F, 6 physical cores / 6 logical (no HT), 2.90 GHz base, 9 MB L3, AVX2/FMA (no AVX-512), integrated UHD 630 (no dGPU), 16 GB RAM, SSD (C:) + HDD (D:)
**Continuity:** This document extends the 3-thread STT pipeline you already scoped (cpal → rubato → silero-vad → whisper-rs, whisper-small GGUF). That pipeline is treated as a subsystem here, not re-derived.

---

## 1. Functional Requirements — Clarified & Confirmed

| ID | Requirement | Status |
|---|---|---|
| FR1 | App launches with no visible console window or dialog; starts in background on demand or at logon | Confirmed from your description |
| FR2 | A circular, borderless, always-on-top visual indicator is shown on screen | Confirmed |
| FR3 | Indicator visually reacts (glow/animate) when VAD detects active speech | Confirmed |
| FR4 | Speech is transcribed and the resulting text is inserted at the **active keyboard input focus** | **Reinterpreted — see note below** |
| FR5 *(inferred, not stated)* | A way to mute/pause listening without closing the app | Recommended addition — see §8 |
| FR6 *(inferred)* | A way to exit the app cleanly (tray context menu) | Recommended addition |
| FR7 *(open)* | Autostart at Windows logon | Needs your confirmation (§10) |

**Important correction on FR4:** you wrote "wherever my mouse cursor... is located." On Windows (and every OS), typed/injected text goes to the **keyboard-focused control** (the caret), not to the mouse pointer's (x, y) coordinate — these are frequently in different places (e.g., your mouse is hovering a browser tab while your caret sits in a chat box below it). I'm treating this as a wording slip rather than a real requirement for pointer-anchored injection, because pointer-anchored text insertion isn't how any text control on Windows works — there's no API to "type at pixel (x,y)" independent of focus. If you actually *did* mean click-to-position behavior (i.e., the app should first click at the pointer to move focus there before typing), flag it — it changes the design (we'd need to synthesize a mouse click before injection, which is riskier: clicking the wrong control, e.g., a button instead of a textbox, triggers unintended actions).

---

## 2. Non-Functional Requirements

| Category | Target | Rationale |
|---|---|---|
| End-to-end latency (speech-end → text appears) | < 1.5 s for a 3–5 s utterance | Perceived "real-time" threshold for dictation UX |
| Idle CPU usage | Near 0% (VAD-gated; audio thread + VAD only) | Background app must not compete with foreground workloads |
| Peak CPU usage (during inference) | ≤ 4 of 6 cores, bounded | Leaves headroom for OS/UI/other apps on a 6C/6T part with no HT |
| Memory footprint | < 500 MB resident (model + buffers + overlay) | Model itself is ~150–500 MB depending on quant; rest should be thin |
| Privacy | 100% local inference, no network calls, no cloud fallback | Matches your existing offline whisper.cpp design intent |
| Reliability | App must never crash the foreground application it types into | Text injection is the highest blast-radius component — see §8 |
| Data retention | No persistent verbatim transcript log by default | See §9 (Security & Privacy) |

---

## 3. High-Level Architecture (textual diagram)

```
┌───────────────────────────────────────────────────────────────────────┐
│                              Windows 10 Host                            │
│                                                                          │
│  Mic ──► [T1 Audio Capture: cpal] ──► [Resample: rubato → 16k mono f32] │
│                                              │                          │
│                                              ▼                          │
│                              [T2 VAD Gatekeeper: silero-vad]            │
│                               (endpointing: pause → freeze buffer)      │
│                                  │                        │            │
│                        VAD prob/state              utterance buffer     │
│                                  │                        │            │
│                                  ▼                        ▼            │
│                        [T4 Main/UI thread]        [T3 ASR: whisper-rs]  │
│                     (Win32 msg loop, overlay,             │            │
│                      tray icon, hotkey, WTS)         raw transcript     │
│                                  ▲                        ▼            │
│                                  │              [Post-process: trim,   │
│                          indicator state ◄───────  capitalize, punct.] │
│                                                             │          │
│                                                             ▼          │
│                                                  [T5 Text Injector]     │
│                                                 (UIA → paste → SendInput│
│                                                  tiered fallback)       │
│                                                             │          │
│                                                             ▼          │
│                                         Focused control in ANY foreground│
│                                              application's window       │
└───────────────────────────────────────────────────────────────────────┘
```

Five logical threads total (T1–T5), up from the three in Doc 1. T4 (main thread) owns everything with Win32 window-message affinity: the overlay window, the tray icon, global hotkey registration, and session lock/unlock notifications. T3 and T5 communicate through a bounded channel so injection never blocks inference.

---

## 4. Component Breakdown

### T1 — Audio Capture *(unchanged from Doc 1)*
- **Crate:** `cpal`. Opens default input device, pushes raw f32 samples into a bounded ring buffer / `crossbeam-channel`.
- **Failure modes:** default device changes mid-session (user unplugs a USB mic) — must handle `cpal::StreamError` and attempt device re-enumeration rather than silently going deaf.

### T2 — VAD Gatekeeper *(unchanged from Doc 1)*
- **Crate:** `silero-vad`. 30 ms analysis windows, hangover/hysteresis (~200–500 ms of trailing silence) before declaring end-of-utterance, to avoid clipping the tail of words.
- **Emits:** a `SpeechState` (Idle / Speaking) on every window to drive the indicator in real time, independent of whether ASR has finished.
- **Failure mode — false triggers:** HVAC noise, keyboard clicks, or a TV in the background can trigger VAD. Because the output of this pipeline is **system-wide text injection**, a false trigger doesn't just waste CPU — it can type garbage into whatever is focused (a chat window, an IDE, a password manager). See §8 for the mitigation (focus-aware gating), which I consider the single most important addition to your original spec.

### T3 — ASR Inference *(unchanged core, timing analysis new)*
- **Crate:** `whisper-rs`, `whisper-small` GGUF (Q8_0 or Q4_K_M), 4 of 6 threads pinned.
- **Chunking strategy:** utterance-level (VAD-endpointed), not sliding-window streaming. See §7 comparison — this is the right default for MVP.
- **Rough latency estimate (unverified, benchmark before committing):** whisper-small quantized on a 4-core AVX2 CPU of this class typically runs at a real-time factor of roughly 0.3–0.6× for short utterances, i.e., a 4 s utterance might take ~1.5–2.5 s to transcribe. This is an estimate from general whisper.cpp community benchmarks on similar hardware, not a measurement on your machine — treat it as a planning assumption to validate in Phase 1, not a spec.

### T4 — Overlay + Tray + Lifecycle (new)
- **Overlay window:** layered (`WS_EX_LAYERED`), topmost (`WS_EX_TOPMOST`), tool window (`WS_EX_TOOLWINDOW`, so it never shows in Alt-Tab or the taskbar), and critically **`WS_EX_NOACTIVATE`** — this ensures that if the user clicks the indicator (e.g., to mute), the click does *not* steal keyboard focus away from whatever text field they were dictating into. Missing this flag is a subtle but very real bug class for always-on-top widgets.
- **Rendering:** recommend **Direct2D** via `windows-rs` bindings — hardware-accelerated on the integrated UHD 630, low CPU, clean anti-aliased circle + glow. See §7 for the full comparison against GDI+/egui/WebView.
- **Tray icon:** `Shell_NotifyIcon` (or the `tray-icon` crate) for a right-click context menu: Mute/Unmute, Settings, Exit.
- **Global hotkey:** `RegisterHotKey` (or `global-hotkey` crate) for a mute/unmute toggle independent of clicking the overlay.
- **Session awareness:** subscribe to `WM_WTSSESSION_CHANGE` (via `WTSRegisterSessionNotification`) to detect lock/unlock. On lock, force-mute the mic — both for privacy and because injected input can't reach the secure desktop anyway (see §8).
- **DPI awareness:** must call `SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)` at startup, or the overlay will render blurry/mis-sized when dragged across monitors with different scaling — an easy-to-miss requirement on any multi-monitor Windows 10 box.

### T5 — Text Injector (new)
- **Responsibility:** takes a finished, post-processed transcript string and delivers it to the currently focused control.
- **Strategy:** tiered fallback (best fidelity first):
  1. **UI Automation** (`ITextRangeProvider` / `ValuePattern.SetValue`) — attempt first if the focused element exposes a usable text pattern.
  2. **Clipboard + simulated Ctrl+V** — save existing clipboard contents, set `CF_UNICODETEXT`, simulate paste, restore clipboard. Best for long utterances.
  3. **`SendInput` with `KEYEVENTF_UNICODE`**, one synthetic keydown/up pair per character — universal fallback, but O(n) in text length and subject to UIPI blocking (below).
- **Known hard limitation — UIPI:** if the foreground app runs at a higher integrity level than your dictation app (most commonly: foreground app is "Run as Administrator", yours is not), Windows' User Interface Privilege Isolation will silently discard your synthetic input. This is a security feature, not a bug you can code around without also running elevated (not recommended — an always-elevated background input-injector is a much larger attack surface and will draw AV attention, see §9). Correct behavior: detect the failure (e.g., via a UIA readback check) and surface an error state on the indicator rather than silently dropping the transcript.

---

## 5. Indicator State Machine

```
        VAD: speech          ASR: dispatched         ASR: done / injected
Idle ───────────────► Speaking ───────────────► Transcribing ───────────────► Idle
  ▲                                                     │
  │                        injection failed / error      │
  └─────────────────────────── Error (flash) ◄───────────┘

Any state ──[hotkey/click: mute]──► Muted (dim, static) ──[unmute]──► Idle
Any state ──[session lock]────────► Muted (forced, until unlock)
```

Five visual states total: Idle, Speaking (pulsing), Transcribing (spinner-like animation), Error (brief red flash, auto-reverts), Muted (dimmed/static). Keeping this to five avoids overloading a single small circular affordance with more meaning than a glance can parse.

---

## 6. End-to-End Data Flow

1. **Input:** raw interleaved PCM from the default input device (typically 44.1/48 kHz, 16/24-bit, mono or stereo depending on the mic).
2. **Normalize:** downmix to mono, resample to 16 kHz f32 (`rubato`) — Whisper's hard requirement.
3. **Gate:** silero-vad scores each 30 ms window; below-threshold windows are dropped (CPU stays idle); above-threshold windows accumulate into an utterance buffer.
4. **Endpoint:** after a hangover period of trailing silence, the buffer is frozen and handed to T3.
5. **Transcribe:** whisper-rs runs greedy decoding over the full utterance (full context, not streaming).
6. **Post-process:** trim leading/trailing whitespace, optional capitalization of sentence starts, optional basic punctuation pass (Whisper's own punctuation output is generally usable as-is for English and can often be trusted rather than re-derived).
7. **Focus-safety check (recommended, see §8):** query the currently focused control's UIA `ControlType`; if it is not an editable text control (Edit, Document, or an equivalent custom pattern), suppress injection and flash the Error state instead of typing into a button, list, or game window.
8. **Inject:** tiered UIA → clipboard-paste → SendInput, as above.
9. **Output:** text appears in the focused control; indicator returns to Idle.

Throughout, indicator state updates flow independently and continuously from T2/T3/T5 to T4, so the visual reacts to VAD activity even while ASR is still catching up on a previous utterance (bounded queue — see §8 backpressure note).

---

## 7. Comparative Analysis

### 7.1 Text Injection Method

| Method | Mechanism | Pros | Cons | Best for |
|---|---|---|---|---|
| `SendInput` (`KEYEVENTF_UNICODE`) | Per-character synthetic keydown/up | Universal compatibility; natural per-char undo granularity | Slow for long text; blocked across UIPI integrity boundaries; can trip autocomplete handlers | Fallback, short utterances |
| Clipboard + simulated paste | Set `CF_UNICODETEXT`, simulate Ctrl+V | O(1) regardless of length; simple | Clobbers user's clipboard (must save/restore); one atomic undo (pro or con depending on taste) | Long-form dictation, default path |
| UI Automation `SetValue`/`ITextRangeProvider` | Direct COM call into the focused control's accessibility tree | No keystroke simulation at all; unaffected by most input blocking | Inconsistent support (many Electron/custom-drawn editors implement UIA text patterns poorly or not at all); higher COM interop complexity | Native Win32/WPF/WinForms apps, Office, Notepad |

**Recommendation:** tiered fallback, attempted in the order above (UIA → clipboard → SendInput), with the tier used surfaced in a debug/tray tooltip for troubleshooting.

### 7.2 Overlay Rendering Approach

| Approach | Footprint | Visual quality | Complexity | Verdict |
|---|---|---|---|---|
| GDI/GDI+ | Very low | Basic, harder to get smooth glow | Low | Fine for a rough MVP |
| **Direct2D** (`windows-rs`) | Low, GPU-accelerated via UHD 630 | High, smooth anti-aliasing/blur | Medium | **Recommended** |
| egui/wgpu (immediate-mode GUI) | Higher | High, flexible | Medium–High | Overkill unless you plan a full settings UI |
| WebView2/Tauri | Highest (spawns a browser engine) | Very high, CSS-easy | High | **Not recommended** — contradicts the "silent, lightweight background app" goal (50–150+ MB overhead for a single animated circle) |

### 7.3 ASR Chunking Strategy

| Strategy | Perceived latency | Accuracy | Complexity | Notes |
|---|---|---|---|---|
| Utterance-level (VAD-endpointed) | ~0.3–1 s pause + full inference | High (full context) | Low | **Recommended for MVP** — matches your existing Doc 1 design |
| Fixed sliding-window streaming | Lower during speech | Lower — boundary word errors, re-decode "flicker" | High | Needs hypothesis-diffing to avoid retyping garbled interim text |
| Two-pass hybrid (tiny model partial + small model final) | Best of both | Medium–High | Very high | Worth prototyping only after MVP UX is validated |

---

## 8. Edge Cases & Failure Modes

- **False VAD triggers injecting unwanted text.** This is the highest-consequence failure mode in the whole system, because the blast radius is *any focused control on the desktop* — a chat window, an IDE, a password field. Mitigation, in order of effectiveness: (a) require the focus-safety UIA check in §6 step 7 (don't inject into non-text controls); (b) consider an explicit "armed" state (hotkey to start/stop dictation sessions) rather than always-hot listening, even though your original description implies always-on VAD-gated capture — this is worth deciding deliberately rather than by default, see §10.
- **UIPI blocking across integrity levels** — synthetic input silently dropped when the foreground app runs elevated. No code-level workaround short of also running elevated (not recommended — see §9).
- **Secure desktop (UAC prompt, Ctrl+Alt+Del, lock screen).** Input injection and the overlay itself cannot reach the secure desktop by OS design — this is a security boundary, not a bug to fix. Correct behavior is to detect session lock and force-mute (§4, T4).
- **Password fields (`ES_PASSWORD`).** Detectable via UIA; recommend refusing to inject (or at minimum warning) into password controls, both to avoid accidental sensitive dictation and to avoid confusing autofill/masking behavior.
- **Full-screen exclusive apps (games, some video players).** Topmost overlays generally cannot render over DirectX exclusive fullscreen surfaces — an OS-level limitation, not something to "fix," just something to document as a known gap.
- **Multi-monitor, mixed DPI.** Without per-monitor-v2 DPI awareness, the overlay will render at the wrong size/position when dragged between monitors of different scale factors.
- **Model/quantization accuracy trade-off.** Q8_0 vs Q4_K_M is effectively a numerical-precision-vs-latency trade-off: Q4_K_M is smaller/faster but has measurably higher WER, especially on non-native accents, domain jargon, or noisy input — worth A/B testing both on your own voice/environment rather than assuming Q8_0 is "safe enough."
- **Distribution shift.** Whisper's training distribution skews toward clean, native-accent, broadcast-style speech. Background noise, code-switching, or heavy domain vocabulary (e.g., dictating code identifiers) will degrade accuracy — worth deciding early whether you need a custom vocabulary/prompt-biasing pass for things like variable names.
- **Backpressure.** If ASR falls behind (e.g., you speak faster than inference completes), the audio/VAD pipeline must not block — use a bounded channel with a drop-oldest or coalesce policy between T2 and T3, and surface an Error/Busy state on the indicator rather than silently queuing an ever-growing backlog.

---

## 9. Security & Privacy Considerations

- **No global keyboard hook needed or wanted.** This design only ever *writes* synthetic input (`SendInput`/UIA); it never needs to *read* real keystrokes via `SetWindowsHookEx(WH_KEYBOARD_LL)`. Deliberately avoid adding one — both to minimize the app's actual privacy footprint and because a hidden background process that combines a keyboard hook + autostart + input simulation is exactly the heuristic signature antivirus/SmartScreen engines flag as a potential keylogger or PUA (potentially unwanted application). Since you don't need the hook for any stated requirement, leaving it out is a genuine risk reduction, not just a nice-to-have.
- **Code signing.** An unsigned .exe that autostarts, runs hidden, and injects input system-wide is a near-textbook AV/SmartScreen false-positive pattern. Budget for a code-signing certificate before distributing this beyond your own machine (even for personal use, Defender may still quarantine it on first run without one).
- **Transcript logging.** Don't persist verbatim dictation history to disk by default — a plaintext log of everything you've ever dictated (which may include passwords typed by mistake, personal messages, etc.) is itself a privacy liability sitting on disk. If you want history for debugging, log timings/error codes/state transitions via `tracing`, not transcript content, unless the user explicitly opts into a history feature.
- **Telemetry.** Given the fully-offline design intent, "observability" here should stay 100% local (rotating log file under `%APPDATA%`) — no remote telemetry endpoint, which is the opposite of typical production advice but correct for this project's privacy posture.

---

## 10. Open Decisions Needing Your Confirmation

1. **Always-hot VAD listening vs. explicit "armed" sessions.** Your description implies always-on (consistent with Doc 1's VAD gatekeeper design), but given the false-trigger blast radius in §8, do you want a hotkey to arm/disarm dictation, or accept always-hot with the focus-safety check as the only guardrail?
2. **Indicator placement/behavior.** You mentioned "near the system tray or floating on the desktop" — I've assumed a draggable floating overlay (default bottom-right, position persisted) *plus* a separate tray icon for the context menu. Confirm, or pick one.
3. **Autostart at logon** — yes/no, and if yes, registry Run key vs. Startup folder shortcut (functionally similar; registry is slightly more common for this class of app).
4. **Language scope** — English-only for now (matches Doc 1's `set_language(Some("en"))`), or multi-language from day one?
5. **History/undo feature** — any interest in a small scrollback of recent transcripts (kept in memory only, not persisted) so you can re-paste or correct a bad transcription?

---

## 11. Suggested Technology Stack (names only, no code)

| Concern | Crate | Purpose |
|---|---|---|
| Win32/COM bindings | `windows` (windows-rs) | Direct2D, UIA, SendInput, Shell_NotifyIcon, WTS notifications |
| Tray icon | `tray-icon` | Cross-platform-ish tray icon + menu, thin wrapper over Shell_NotifyIcon |
| Global hotkey | `global-hotkey` | Mute/unmute toggle |
| Clipboard | `arboard` | Save/set/restore clipboard content for paste-injection |
| Single-instance guard | `single-instance` (or manual `CreateMutexW`) | Prevent duplicate launches |
| Autostart | `auto-launch` | Registry Run-key management |
| Inter-thread messaging | `crossbeam-channel` | Bounded channels between T1–T5 |
| Config | `serde` + `toml` | Overlay position, mute state, language, hotkey binding |
| Logging | `tracing` + `tracing-appender` | Local rotating log, metadata-only per §9 |
| *(carried from Doc 1)* | `cpal`, `rubato`, `silero-vad`, `whisper-rs` | Audio capture, resample, VAD, ASR |

---

## 12. Phased Roadmap

- **Phase 1 (MVP):** T1–T3 exactly as Doc 1 scoped it, plus a minimal Direct2D circle overlay with 3 states (Idle/Speaking/Transcribing) and clipboard-paste-only injection (skip UIA and the SendInput fallback initially — clipboard alone covers the large majority of real apps and is the fastest path to a usable prototype).
- **Phase 2:** add the UIA tier and SendInput fallback (tiered injection), focus-safety check, tray menu, mute hotkey, session-lock awareness, DPI awareness.
- **Phase 3 (optional):** two-pass streaming for lower perceived latency, custom vocabulary/prompt biasing for domain jargon, code signing + distribution polish.

---

## 13. Executive Summary

The ASR core (Doc 1) is solid and needs no changes. The genuinely new engineering surface is the **overlay + text-injection layer**, and its central risk isn't ASR accuracy — it's that a false VAD trigger combined with system-wide text injection can silently corrupt whatever the user is focused on. The single highest-leverage addition to your original spec is a **focus-safety check** (only inject into verified editable-text controls) before any injection path fires. Beyond that: use tiered injection (UIA → clipboard → SendInput), Direct2D for the overlay, avoid any keyboard *hook* (you only need to write input, never read it — both for privacy and to avoid AV false positives), and treat "real-time" as utterance-level with ~0.3–1.5 s perceived latency rather than true word-by-word streaming, which is a much harder and lower-accuracy problem not worth taking on for an MVP.
