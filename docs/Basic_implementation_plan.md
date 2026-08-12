# Phase 1 Basic Implementation

Phase 1 implements a platform-independent local microphone capture and VAD
pipeline. The same core is intended to run on Windows and Ubuntu/Linux.

## Locked decisions

- `EnergyVad` is the development and test implementation.
- `SileroVad` is the production implementation target behind the same `Vad` abstraction.
- Audio is captured locally through `cpal` and normalized to 16 kHz mono `f32`.
- Device access uses `cpal`'s native host backend; no Windows-only audio API is
  used by the shared pipeline.
- Audio is processed in 30 ms frames through bounded worker channels.
- OS-specific UI, focus, injection, tray, hotkey, autostart, and lock/session
  services are deferred to platform adapters.
- No ASR, overlay, target resolution, clipboard, SendInput, or transcript persistence is included.

## Implemented boundaries

- `audio_capture`: default microphone discovery, stream lifecycle, bounded callback channel.
- `preprocessing`: sample normalization, mono conversion, linear resampling, and frame assembly.
- `vad`: `Vad` trait, `EnergyVad`, speech state transitions, hangover, pre-roll, and utterance events.
- `session`: lifecycle state vocabulary reserved for start/stop integration.
- `platform`: platform identity only; it contains no OS-native handles.

## Production follow-up

Replace the VAD implementation with `SileroVad` without changing capture or preprocessing interfaces, then add ASR as the next pipeline stage.

When platform services are added, expose narrow shared interfaces and provide
separate Windows and Linux implementations. Linux support must later identify
X11 and Wayland as separate compatibility targets.
