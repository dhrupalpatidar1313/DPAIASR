# Voice Dictation — Final Consolidated Architecture

**Status:** Authoritative. Supersedes all prior drafts (the original Windows-only spec, its v2 revision, your uploaded session/injection refinement, and the cross-platform proposal). This document reconciles all four into one plan and resolves the one direct conflict found between them.
**Scope:** Windows 10 x64 (full) + Ubuntu LTS / X11 (full) in the initial release. Wayland is explicitly deferred — see §5, this is a flagged assumption from "build both now," not something you confirmed at the X11/Wayland granularity, so correct it if wrong.
**Language/runtime:** Rust. Shared platform-neutral core + trait-based platform adapters (Windows, Linux/X11 now; Linux/Wayland later).

---

## 1. Product Scope & Platform Targets

| Target | Status | Note |
|---|---|---|
| Windows 10 x64 | Build now | Full feature set |
| Ubuntu LTS + X11 | Build now | Full feature set, near parity with Windows |
| Ubuntu LTS + Wayland | Deferred (Phase 2+) | See §5 — fundamentally different permission model, not just a different API |

---

## 2. Functional Requirements — Final

| ID | Requirement | Notes |
|---|---|---|
| FR1 | Silent background launch, no console/dialog | |
| FR2 | Circular always-on-top overlay indicator + tray icon for exit | Tray availability differs by platform — see §12 |
| FR3 | Indicator reacts to VAD/processing activity via layered color+animation | See §10 |
| FR4 | Text is inserted at keyboard focus/caret, never mouse pointer position | Confirmed via your IDE/WhatsApp Web examples |
| FR5 | Session Start/Stop is dual-triggered: clicking the overlay, **or** a global hotkey — both call the same underlying toggle, not two separate features | Finalized this round |
| FR6 | Clean exit via tray context menu | |
| FR7 | No autostart | |
| FR8 | One session = many independent utterances, split by VAD pauses; each transcribed and injected independently, not batched | |
| FR9 | Stop halts new capture immediately but lets any in-flight utterance drain and inject normally | |
| FR10 | **Invariant:** a transcript belongs to the target captured when its utterance ended — never to whatever is focused when ASR finishes. No automatic focus-stealing, ever. | Locked, non-negotiable per your explicit answer earlier |
| FR11 | When the captured target can't be safely used, the transcript goes into a bounded FIFO pending queue with automatic + manual retry; only falls back to clipboard as a terminal safety net if retries are exhausted | Revised this round — see §9 |
| FR12 | ASR language: English only for now | |
| FR13 | Hard ceiling on a single utterance's duration (`MAX_UTTERANCE_DURATION`), independent of VAD hangover, so continuous unbroken speech can't produce an unbounded buffer | Added from your uploaded refinement |
| FR14 | Password/secure fields are always refused for injection — no "probably safe" heuristic, refuse and flag | Strengthened per your uploaded refinement |
| FR15 | Core engine behavior (session/utterance policy, target/injection policy, pending-queue policy) is identical on both platforms; only the platform adapter layer differs in what it can actually deliver | New — the cross-platform contract |

---

## 3. Non-Functional Requirements

| Category | Target | Platform notes |
|---|---|---|
| End-to-end latency (utterance end → injected) | < 1.5 s typical | Same core pipeline both platforms; Windows/X11 roughly at parity |
| Idle CPU (session stopped) | Near 0% | |
| Peak CPU (inference) | ≤ 4 of 6 cores | |
| Memory | < 500 MB resident | |
| Privacy | 100% local inference, no network calls | |
| Correctness priority | Never wrong-window injection, never silent focus steal, even at the cost of falling back to pending | This is the single hardest invariant in the whole system — everything else bends around it |
| Cross-platform parity | Windows and Ubuntu/X11 should feel like the same product; Wayland is explicitly allowed to be a reduced-capability tier, not silently broken | |

---

## 4. High-Level Architecture

```
┌───────────────────────────────────────────────────────────────────┐
│                    Platform-Neutral Core (Rust)                    │
│                                                                      │
│  Audio Capture ─► Preprocess ─► VAD/Endpointing ─► Utterance Mgr    │
│                                                          │           │
│                                                          ▼           │
│                                              ASR Queue (whisper-rs)  │
│                                                          │           │
│                                                          ▼           │
│                                          Transcript Processor        │
│                                                          │           │
│                                                          ▼           │
│                                     Target Resolution Policy         │
│                                     (snapshot @ utterance-end,       │
│                                      revalidate @ inject-time)       │
│                                              │            │          │
│                                          valid        invalid        │
│                                              │            │          │
│                                              ▼            ▼          │
│                                   Injection Policy   Pending Queue   │
│                                   (capability-based   (bounded FIFO, │
│                                    tier selection)     auto+manual   │
│                                              │          retry)       │
│                                              │            │          │
│                                              └─────┬──────┘          │
│                                                     │                │
│                                          Session Controller          │
│                                    (Start/Stop, STOPPING drain,      │
│                                     indicator state, backpressure)   │
└──────────────────────────────────┬───────────────────────────────┘
                                    │ PlatformServices trait boundary
                    ┌───────────────┴───────────────┐
                    │                               │
          ┌─────────▼─────────┐           ┌─────────▼─────────┐
          │  Windows Adapter   │           │  Linux/X11 Adapter │
          ├────────────────────┤           ├────────────────────┤
          │ Overlay (Direct2D) │           │ Overlay (X11)      │
          │ Tray (Shell_Notify)│           │ Tray (SNI, if DE   │
          │ Hotkey (RegisterHK)│           │  supports it)      │
          │ Focus (Win32+UIA)  │           │ Hotkey (XGrabKey)  │
          │ Inject: UIA/paste/ │           │ Focus (X11+AT-SPI) │
          │   SendInput        │           │ Inject: AT-SPI/    │
          │ Autostart (Registry)│          │   XTest+paste      │
          │ SessionMonitor(WTS)│           │ Autostart (XDG)    │
          └────────────────────┘           │ SessionMonitor     │
                                            │  (D-Bus/logind)    │
                                            └────────────────────┘
```

---

## 5. Why Wayland Is Scoped Separately

"Build both now" is treated here as **Windows + Ubuntu/X11**, not Windows + Ubuntu/(X11 or Wayland). This isn't a difficulty judgment call I'm deferring arbitrarily — it's that Wayland changes the *nature* of several requirements, not just their implementation:

- **Overlay positioning:** `wlr-layer-shell` (the protocol that would let an app place an always-on-top, non-stealing overlay) is supported by wlroots-based compositors, generally not by GNOME/Mutter — Ubuntu's default.
- **Tray icons:** GNOME has no built-in StatusNotifierItem host; it requires a user-installed shell extension. "No tray" isn't a bug to fix, it's the default state of the target desktop.
- **Global hotkeys and injection:** both route through `xdg-desktop-portal`, which is **permission-gated per session** — the user sees an OS-level consent prompt, potentially every session, rather than the silent background operation that both Windows and X11 provide.

That last point is the real reason this isn't just "more platform code": on Wayland, "Pending" (our answer for a transient focus race) and "not achievable without an explicit permission grant" (Wayland's actual constraint) are different UX categories, and conflating them would misrepresent what the app can promise. Recommend treating Wayland as Phase 2, evaluated fresh once X11 + Windows are solid — flag now if you want it pulled into scope 1 instead.

---

## 6. Session/Utterance Model *(unchanged, now core-level)*

```
Session:  START → ACTIVE → STOPPING → STOPPED
Utterance (inside an ACTIVE session): speech → pause → complete → ASR → target-resolve → inject
```

One click (or hotkey press — see FR5) per session, not per sentence. VAD owns all utterance boundaries inside it. This lives entirely in the platform-neutral core; neither OS needs to know it exists.

---

## 7. State Machine (finalized)

Two independent axes, exactly as agreed:

**Session axis (persistent color):** `STOPPED` (grey) → `LISTENING` (blue)
**Activity axis (animation overlay, layered on top):** none / pulse (speaking) / spin (transcribing) / slow-pulse (item sitting in pending queue) / flash (error) / small ring (`STOPPING` — draining last utterance after Stop)

```
STOPPED ──start──► LISTENING ──speech──► SPEAKING ──pause──► TRANSCRIBING
   ▲                  │                                          │
   │                  │◄─────────────── inject success ──────────┘
   │                  │
   │             (Stop clicked while draining)
   │                  ▼
   └──────────── STOPPING ──(drain complete)──► STOPPED

TRANSCRIBING ──target invalid──► PENDING ──retry succeeds──► LISTENING
                                     │
                            retries/TTL exhausted
                                     ▼
                          ERROR flash → clipboard fallback → STOPPED/LISTENING
```

Note: a prior draft had a separate `MUTED` state. That's removed — with click/hotkey-toggle replacing the old mute concept, and system-lock forcing a `STOPPED` transition (§13), a separate Muted state was redundant with Stopped and is folded into it.

Because hotkey and overlay-click both call the same `SessionController::toggle()`, the indicator's state logic never needs to know which one fired — it just observes session state. That's a direct consequence of FR5 being "one control, two input paths" rather than two features.

---

## 8. Injection Strategy — Capability-Based Tiering (reframed per your uploaded refinement)

Not a fixed "always try UIA, then clipboard, then SendInput" sequence — tier selection depends on target capability *and* whether focus has moved:

| Tier | Windows | Linux/X11 | Used when |
|---|---|---|---|
| 1 — background-capable | UI Automation `SetValue` | AT-SPI direct text write | Target changed focus, but exposes a writable accessibility interface — can inject without touching current focus |
| 2 — focus-dependent | Clipboard + simulated paste (primary); `SendInput` as last-resort same-focus fallback | Clipboard (X11 selection) + simulated paste via XTest; XTest key events as last-resort fallback | Target is still the currently focused control — most common case |
| 3 — deferred | Pending Queue | Pending Queue | Target changed *and* Tier 1 is unavailable/unreliable for that control type (this is the expected, not exceptional, case for Electron/browser apps on both platforms — VS Code and WhatsApp Web included) |

Same caveat applies on Linux as on Windows: Electron apps and browser `contenteditable` regions have inconsistent AT-SPI support, just as they do with UIA — so Tier 3 should be expected as routine for your two named real-world targets on either OS, not a rare corner case.

---

## 9. Pending Queue — Final Design (resolves the FIFO-vs-clipboard question)

These aren't competing designs — clipboard is now the **terminal fallback beneath** the FIFO queue, not an alternative to it:

- **Bounded FIFO**, capacity `MAX_PENDING` (suggested default: 5, configurable later).
- **Each item independently revalidated** before every retry attempt — never batch-inject the whole queue on one "target is back" signal.
- **Retry triggers:** (a) automatic, on a target-availability signal (focus-change event where the platform can supply one, or lightweight polling otherwise); (b) manual, via a user-initiated retry (e.g., right-click tray → "Retry pending").
- **Retry ceiling per item:** `MAX_RETRY_ATTEMPTS` (suggested default: 3) or a TTL (suggested default: 5 minutes), whichever comes first. On exceeding either, the item is evicted from the queue, **auto-copied to the clipboard** as a last-resort preservation of the content, and the indicator flashes Error — this is exactly the clipboard behavior you originally asked for, just scoped to "recovery of last resort" instead of "default recovery path."
- **Queue overflow** (a 6th item arrives while 5 are pending): evict the oldest per FIFO discipline, auto-copy *it* to clipboard before eviction, then accept the new item. Nothing is ever silently deleted without at least landing on the clipboard first.

This means: the common case (a single popup interruption) resolves invisibly via auto-retry, exactly as your uploaded refinement wanted. The rare case (target never comes back, or the queue backs up) degrades to exactly the clipboard-copy safety net you asked for earlier. Both answers are satisfied because they were never actually about the same layer of the system.

---

## 10. Component Breakdown

### Core (platform-neutral, built and tested once)
- **Audio Capture / Preprocess** — `cpal`, resample to 16k mono f32. Cross-platform by construction (this is `cpal`'s job).
- **VAD / Utterance Manager** — `silero-vad`, hangover endpointing, `MAX_UTTERANCE_DURATION` cap, emits utterance boundaries independent of platform.
- **ASR Queue** — `whisper-rs`, whisper-small GGUF, serial FIFO, separate from the injection/pending queue (two distinct queues, not conflated).
- **Transcript Processor** — trim/normalize only; explicitly *not* an LLM cleanup pass (added latency/complexity/failure surface for no MVP-critical benefit, per your uploaded refinement).
- **Target Resolution Policy** — owns the snapshot-at-utterance-end / revalidate-at-inject-time logic, operating only on the platform-neutral `TargetSnapshot` type (below) — never on raw HWND/AT-SPI types directly.
- **Injection Policy** — tier-selection logic from §8, expressed generically; the actual tier 1/2/3 mechanics are platform calls.
- **Pending Queue** — §9's logic, entirely platform-neutral (it just needs a `retry(target) -> InjectionResult` callback from the platform layer).
- **Session Controller** — Start/Stop/STOPPING lifecycle, backpressure ("Busy" state instead of silently dropping utterances when the ASR queue is saturated), drives indicator state.

### Platform-neutral data contract
```text
TargetSnapshot {
    platform_target_id      // opaque handle, meaningless to core
    process_id
    control_kind            // Editable | Password | NonEditable | Unknown
    captured_at
    capabilities             // e.g. supports_background_write: bool
}

InjectionResult { Success | Retryable | Unsupported | Failed }
```
Core code only ever branches on these — never on `HWND`, `UIA element`, `X11 Window`, or `AT-SPI object`. Those stay entirely inside each platform adapter.

### Platform adapter trait surface (both OSes implement all of these)
```text
trait Overlay { show(); hide(); update_state(session, activity); set_position(); }
trait Tray { show(); update(); menu(); }
trait HotkeyManager { register(); unregister(); on_trigger() -> fires SessionController::toggle() }
trait TargetResolver { capture_target() -> TargetSnapshot; validate_target(TargetSnapshot) -> TargetStatus; }
trait TextInjector { inject(TargetSnapshot, text) -> InjectionResult; }
trait AutostartManager { enable(); disable(); is_enabled() -> bool; }  // present in the trait set even though FR7 = disabled by default; kept for possible future use
trait SessionMonitor { on_lock(); on_unlock(); }
```

### Thread allocation (consolidated per your uploaded refinement — not one OS thread per logical component)
- **Audio thread** — capture callback, feeds ring buffer.
- **ASR worker thread** — VAD + utterance assembly + whisper-rs inference (VAD is cheap enough to share this thread rather than dedicating one).
- **Main/UI thread** — Win32 message loop (Windows) or the equivalent X11 event loop (Linux); owns overlay, tray, hotkey dispatch, session state.
- **Injection/COM worker (Windows only, conditional)** — only spun up if UI Automation's threading model actually requires a dedicated apartment thread; otherwise injection calls happen inline from the main thread's event handling. Linux/X11 has no equivalent COM-apartment constraint.

---

## 11. Repository Structure

```text
src/
├── core/
│   ├── session.rs        // Start/Stop/STOPPING lifecycle
│   ├── utterance.rs       // VAD-driven boundary + MAX_UTTERANCE_DURATION
│   ├── pipeline.rs        // wires audio → VAD → ASR
│   ├── transcript.rs      // minimal post-processing
│   ├── target_policy.rs   // snapshot/revalidate invariant (FR10)
│   ├── injection_policy.rs // tier selection (§8)
│   ├── pending.rs         // bounded FIFO + retry/TTL (§9)
│   └── state.rs           // two-axis indicator state (§7)
│
├── audio/   (capture.rs, preprocess.rs, device.rs)
├── vad/     (trait.rs, silero.rs)
├── asr/     (whisper.rs)
│
├── platform/
│   ├── mod.rs             // PlatformServices trait bundle
│   ├── windows/  (overlay.rs, tray.rs, hotkey.rs, focus.rs, injector.rs, session.rs, autostart.rs)
│   └── linux_x11/ (overlay.rs, tray.rs, hotkey.rs, focus.rs, injector.rs, session.rs, autostart.rs)
│   // linux_wayland/ deliberately absent for now — see §5
│
├── config/
├── logging/
└── main.rs
```

---

## 12. Platform Feasibility Matrix

| Capability | Windows | Ubuntu/X11 | Ubuntu/Wayland (deferred) |
|---|---|---|---|
| Always-on-top, non-activating overlay | Yes (`WS_EX_TOPMOST`/`NOACTIVATE`) | Yes (X11 window hints) | Compositor-dependent, generally no on GNOME |
| Tray icon | Yes (`Shell_NotifyIcon`) | Yes if DE supports StatusNotifierItem | Not by default on GNOME without an extension |
| Global hotkey | Yes (`RegisterHotKey`) | Yes (`XGrabKey`) | Portal-gated, per-session permission prompt |
| Background injection (Tier 1) | UIA, unreliable on Electron/browser | AT-SPI, same reliability caveat | Not generally available |
| Focus-dependent injection (Tier 2) | Clipboard/paste, SendInput | Clipboard/paste, XTest | Portal-gated |
| Session lock detection | `WTSRegisterSessionNotification` | D-Bus/logind signals | Available, less of an issue than the above |
| Autostart | Registry Run key (disabled per FR7) | XDG autostart `.desktop` (disabled per FR7) | Same as X11 |

---

## 13. Edge Cases & Failure Modes (consolidated)

- **Multi-utterance burst outpaces ASR:** utterances queue in the ASR queue (FIFO, separate from the pending/injection queue); when the queue hits capacity, session enters a `Busy` sub-state that temporarily stops accepting new speech rather than silently dropping words — restart accepting once the backlog clears.
- **`MAX_UTTERANCE_DURATION` exceeded:** force an endpoint even without a detected pause, so one continuous monologue can't produce an unbounded buffer.
- **Pending queue overflow / retry exhaustion:** handled by §9 — oldest evicted item (or any item hitting its retry ceiling) is copied to clipboard before being dropped from the active queue, never silently discarded.
- **Password/secure fields:** always refused, no exception, flagged with an error state (FR14) — stronger than "probably safe."
- **Electron/browser targets on either OS:** expect Tier 3 (pending) as the routine outcome on any focus change mid-transcription for VS Code and WhatsApp Web specifically, not an edge case to eliminate.
- **UIPI (Windows) / permission boundaries (Linux, especially future Wayland):** both represent the same underlying category — the OS refusing input delivery across a privilege or security boundary. Neither should be "worked around" by elevating the app or requesting broader permissions than necessary; both should surface as a clean failure into the pending/error path.
- **Secure desktop / lock screen (either OS):** session forced to `STOPPED` on lock; no injection attempted through a secure/locked session boundary — this is an OS security feature, not a bug to route around.
- **Cross-platform accessibility-API exposure:** both UIA (Windows) and AT-SPI (Linux) are, by design, readable by other accessibility consumers (real screen readers, other automation tools) — low risk, but worth knowing once the app is cross-platform rather than assuming it's a Windows-only consideration.

---

## 14. Security & Privacy *(unchanged core position, restated for both platforms)*

- 100% local inference, no network calls, no telemetry leaving the device.
- No global keyboard-read hook on either platform — the app only ever writes synthetic input, never reads real keystrokes. This is both a genuine privacy minimization and, on Windows specifically, a meaningful reduction in the "looks like a keylogger" heuristic surface that triggers AV/SmartScreen false positives; worth carrying the same restraint to Linux even though Linux AV heuristics are less of a concern there.
- No persistent verbatim transcript log; the only place a transcript's content is ever written to a shared surface is the clipboard, and only as the terminal fallback in §9 — worth remembering that Windows Clipboard History (Win+V) or a Linux clipboard manager could retain it longer than the moment it's copied.
- Code signing recommended for the Windows build before any distribution beyond your own machine, for the reasons above.

---

## 15. Phased Roadmap (revised for parallel Windows + Ubuntu/X11)

- **Phase 0:** Repository skeleton — `core/` and `platform/` from day one, per §11. Don't build a Windows-only pipeline and port later.
- **Phase 1:** Core pipeline (audio → VAD → ASR → transcript) — built and tested once, exercised by both platform adapters via mocked `PlatformServices` before either real adapter exists.
- **Phase 2:** Windows adapter — overlay, tray, hotkey, focus/target resolution, injection tiers, autostart (implemented but disabled per FR7), session-lock.
- **Phase 3:** Linux/X11 adapter — same trait surface, X11/AT-SPI/XTest equivalents.
- **Phase 4:** Pending queue + retry/TTL logic (§9) integrated against both adapters; this is where FR10/FR11's correctness guarantees get validated end-to-end on both OSes.
- **Phase 5:** Cross-platform test pass — same end-to-end scenarios (including the "popup steals focus mid-ASR" case) run on Windows and Ubuntu/X11.
- **Phase 6 (stretch, not in initial scope):** Wayland adapter, revisited fresh given §5's constraints — likely a reduced-capability tier rather than full parity.
- **Phase 7 (polish):** two-pass streaming for lower perceived latency, custom vocabulary, code signing (Windows) / packaging (`.deb`/AppImage for Ubuntu).

---

## 16. Confirmed Decisions Log

| Decision | Resolution |
|---|---|
| FR4 interpretation | Keyboard focus/caret, not mouse pointer |
| Listening control | Click-to-toggle **and** global hotkey, same underlying toggle |
| Autostart | Disabled |
| Utterances within a session | Multiple, independently injected as ready |
| Stop-mid-transcription | Last utterance finishes and injects |
| Focus-change during ASR | Never steal focus, never wrong-window inject; revalidate, else queue |
| Pending-transcript recovery | Bounded FIFO auto+manual retry; clipboard is the terminal fallback, not the default path |
| Language | English only |
| Indicator model | Color = session on/off, animation = activity; `MUTED` folded into `STOPPED` |
| Cross-platform scope | Windows + Ubuntu/X11 built together now; Wayland deferred |
| Thread model | Consolidated (Audio, ASR worker, Main/UI, conditional COM worker) rather than one thread per logical component |
| Injection tiering | Capability-based selection, not a fixed always-UIA-first sequence |

---

## 17. Executive Summary

The final architecture is a platform-neutral Rust core (audio/VAD/ASR/session/target-policy/pending-queue) sitting behind a narrow `PlatformServices` trait boundary, with Windows and Ubuntu/X11 adapters built in parallel for the initial release and Wayland deliberately deferred rather than silently under-supported. The hardest invariant in the system — never steal focus, never inject into the wrong window — is now satisfied by a bounded FIFO pending queue that auto-retries against the correct target and only degrades to a clipboard-copy safety net once retries are genuinely exhausted, which reconciles what looked like two competing designs into one layered mechanism. The single biggest remaining technical risk isn't ASR accuracy or even cross-platform portability in the abstract — it's that your two real target applications (an Electron-based IDE and a browser-based chat client) sit in the one category, on *either* OS, where background injection without focus-stealing is structurally unreliable, so "held pending" should be treated as expected steady-state behavior for those targets, not a bug to chase away.
