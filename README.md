# DPAIASR

[![CI](https://github.com/dhrupalpatidar1313/DPAIASR/actions/workflows/ci.yml/badge.svg)](https://github.com/dhrupalpatidar1313/DPAIASR/actions/workflows/ci.yml)

**Private, local voice dictation.** Press a key, speak, and your words are typed into whatever app has focus. Speech recognition runs entirely on your machine with [Whisper](https://github.com/openai/whisper): no cloud, no account, no audio ever leaves your computer.

## Features
- **Dictate anywhere**: transcribed text is typed into the currently focused window.
- **Fully offline**: Whisper via [whisper.cpp](https://github.com/ggml-org/whisper.cpp) plus [Silero VAD](https://github.com/snakers4/silero-vad) for speech detection.
- **Runs on modest hardware**: the default `small.en` (quantized, ~180 MB) model works on CPU; optional CUDA/Vulkan builds for GPUs.
- **Smart queueing**: if you switch windows mid-dictation, text waits for the original target instead of landing in the wrong app.
- **Orphan recovery**: if the target window closes before transcription finishes, the text is kept and can be copied to the clipboard.

## Platform support

| Platform | Typing into apps | Status overlay | Notes |
|---|---|---|---|
| Linux, GNOME / KDE (Wayland) | Yes, via the desktop portal | GNOME: no (logged instead) | Verified on Ubuntu + GNOME |
| Linux, wlroots (Sway, Hyprland) | Yes, via `wtype` | Yes | Not verified on real hardware |
| Linux, X11 | Yes, via XTest | No (logged instead) | Not verified on real hardware |
| Windows 10/11 | Yes, via `SendInput` | Yes (Direct2D) | Implemented, not verified recently |
| macOS | No | No | Not supported |

---

## Quick start (Linux)

**1. Install build dependencies** (Ubuntu / Debian):
```bash
sudo apt install build-essential pkg-config cmake clang libclang-dev \
    libgtk-4-dev libgtk4-layer-shell-dev libasound2-dev libxkbcommon-dev
# wlroots compositors (Sway, Hyprland) only:
sudo apt install wtype
```
Fedora: `gtk4-devel gtk4-layer-shell-devel alsa-lib-devel libxkbcommon-devel cmake clang`. Arch: `gtk4 gtk4-layer-shell alsa-lib libxkbcommon cmake clang`.

You also need [Rust](https://rustup.rs) 1.88 or newer.

**2. Fetch the VAD model** (one small file):
```bash
./scripts/download_models.sh
```

**3. Run it:**
```bash
cargo run --release -- --listen
```
The Whisper model (~180 MB) is downloaded and checksum-verified automatically on first launch into `assets/models/`.

**4. Allow keyboard access (GNOME / KDE Wayland).** On first launch the desktop shows a one-time *allow keyboard control* dialog. Click **Allow**. The permission is remembered in `~/.config/dpaiasr/`, so there is nothing to configure and no `sudo` involved. If you deny it, each transcript is copied to the clipboard instead and you paste it with `Ctrl+V`.

Click into any text field, speak, and pause. Text appears after a few seconds (see [Performance](#performance)).

## Quick start (Windows)
1. Install [Rust](https://rustup.rs). *(Optional, GPU)* install the NVIDIA CUDA Toolkit.
2. Fetch the VAD model: download `silero_vad.onnx` (see `scripts/download_models.sh` for the URL) into `assets/models/`.
3. Build and run:
   ```powershell
   ./scripts/build.ps1          # uses CUDA automatically if nvcc is found
   # or: cargo run --release -- --listen
   ```

---

## Usage

### Controls
| Control | Action |
|---|---|
| `Alt + M` | Toggle listening |
| `Alt + R` | Retry text that is waiting for its target window |
| `Alt + C` | Copy the latest orphaned transcript to the clipboard |
| `Enter` or `t` (in the terminal running the app) | Toggle listening |
| `r` / `c` (in the terminal) | Retry / copy, same as the hotkeys |
| `Ctrl + C` (in the terminal) | Quit |

Global hotkeys do not work on most Wayland sessions; use the terminal controls there.

### Command-line options
| Option | Description |
|---|---|
| `--listen`, `-l` | Start in listening mode instead of stopped |
| `--model <name>`, `-m` | `tiny`, `base`, `small` (default, quantized), `small.en`, `medium` |

Logging uses `RUST_LOG`, e.g. `RUST_LOG=debug cargo run --release` to see voice-detection levels.

### Status indicator
On compositors with layer-shell support (Sway, Hyprland, KDE) and on Windows a small circle shows the state: grey stopped, green listening, blue speaking, amber transcribing, orange waiting for target, purple orphaned, red error. On GNOME the same state changes are written to the log.

## Performance
Transcription on CPU is roughly real-time (a 3-second phrase takes about 3-4 seconds on a laptop CPU). For faster results use `--model tiny` or `--model base`, or build with GPU support:
```bash
cargo build --release --features cuda      # NVIDIA, needs the CUDA toolkit
cargo build --release --features vulkan    # Vulkan-capable GPUs
```
Small GPUs (about 2 GB of VRAM) are better served by the default `small` model than by `medium`.

## Troubleshooting
- **Nothing is transcribed**: run with `RUST_LOG=debug` and check that `speech probability` rises when you talk. Check your default microphone and its input level in system settings.
- **`Silero VAD unavailable, falling back to EnergyVad`**: run `./scripts/download_models.sh`.
- **Text is copied instead of typed**: keyboard permission was denied or the portal is unavailable. Delete `~/.config/dpaiasr/portal-restore-token` and restart to be asked again.
- **Wayland, no overlay**: expected on GNOME, which does not implement layer-shell.

## Development
```bash
./scripts/download_models.sh     # needed by the VAD tests
cargo test                       # unit + integration tests
cargo test --release -- --ignored   # also runs the Whisper transcription test (slow, needs the model)
```
Design notes live in [`docs/`](docs/). Contributions are welcome: see [CONTRIBUTING.md](CONTRIBUTING.md).

## Acknowledgements
[whisper.cpp](https://github.com/ggml-org/whisper.cpp) / [whisper-rs](https://github.com/tazz4843/whisper-rs), [Silero VAD](https://github.com/snakers4/silero-vad), [ONNX Runtime](https://onnxruntime.ai) (`ort`), [ashpd](https://github.com/bilelmoussaoui/ashpd), [enigo](https://github.com/enigo-rs/enigo).

## License
MIT: see [LICENSE](LICENSE). Whisper models and Silero VAD are downloaded separately and carry their own (MIT) licenses.
