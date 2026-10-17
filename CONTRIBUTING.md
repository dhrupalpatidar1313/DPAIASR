# Contributing to DPAIASR

Thanks for your interest! Bug reports, platform testing and pull requests are all welcome.

## Getting set up
Follow the **Quick start** in the [README](README.md) to install the build dependencies and run `./scripts/download_models.sh`. Then:

```bash
cargo build
cargo test                          # needs silero_vad.onnx from the script above
cargo test --release -- --ignored   # optional: real Whisper transcription (slow, downloads a model)
```

Rust 1.88 or newer is required.

## Before you open a pull request
- Open an issue first for anything larger than a small fix, so we can agree on the approach.
- Keep changes focused: one concern per PR.
- Make sure `cargo build` is warning-free and `cargo test` passes.
- Add or update tests for behavior you change, and update the README if user-facing behavior changes.
- Platform-specific code lives under `src/platform/<os>/`; keep the rest of the code platform-neutral.

## Testing on other platforms
Windows, X11 and wlroots (Sway/Hyprland) support exists but has had little real-world testing. Reports of what works and what doesn't are very valuable; please include your OS, compositor/desktop and the log output (`RUST_LOG=debug`).

## Commit style
Short, imperative subject line ("Add X", "Fix Y"), with a body explaining *why* when it isn't obvious.

## Project layout
| Path | Purpose |
|---|---|
| `src/audio/` | Microphone capture and resampling |
| `src/vad/` | Voice-activity detection (Silero, energy fallback) |
| `src/asr/` | Whisper engines, model download and verification |
| `src/injection/` | Queueing and delivering text to the target window |
| `src/platform/` | OS-specific hotkeys, overlay, focus tracking, typing |
| `docs/` | Design notes |

## License
By contributing you agree that your contributions are licensed under the [MIT License](LICENSE).
