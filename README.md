# DPAIASR (Dictation & Audio AI ASR)

A high-performance, cross-platform, privacy-first voice dictation utility written in Rust. DPAIASR uses local Whisper AI models to provide seamless voice typing anywhere on your operating system, without requiring a cloud connection.

## ✨ Features
- **Global Voice Dictation**: Press a hotkey to start speaking, and the transcribed text is automatically typed directly into your currently focused application.
- **Cross-Platform**: Designed with a unified trait-based architecture supporting Windows natively and Linux (Wayland) natively.
- **GPU Acceleration**: Automatically detects NVIDIA GPUs to run the highly accurate `medium.en` Whisper model.
- **Graceful CPU Fallback**: Automatically falls back to the lightweight `small.q5_1` model if no GPU is found, ensuring it runs on any hardware.
- **Smart Queueing**: If you switch windows while dictating, DPAIASR ensures the text is sent to the correct application you were originally speaking into.
- **Orphan Recovery**: If the target window is closed before transcription finishes, the text is held in a secure clipboard queue and can be recovered easily.

---

## ⌨️ Usage & Keybindings

Once the application is running, use the following global hotkeys:

| Shortcut | Action | Description |
|---|---|---|
| `Alt + M` | **Toggle Listening** | Start or stop the microphone. Speak while the indicator is green. |
| `Alt + C` | **Recover Orphaned Text** | If the target window closes, press this to copy the transcribed text to your clipboard. |
| `Alt + R` | **Retry Injection** | Retry sending the text if the last injection failed. |

*You will see a floating, transparent circle overlay in the bottom right corner of your screen indicating the current state (e.g., Green = Listening, Yellow = Transcribing, Blue = Speaking, Red = Error).*

---

## ⚙️ Setup & Installation

### Windows 10/11
The Windows build is self-contained and requires minimal setup. It natively uses Direct2D for the UI overlay and the `SendInput` API for text injection.

1. Install Rust (`rustup`).
2. **(Optional) GPU Acceleration**: Install the [NVIDIA CUDA Toolkit](https://developer.nvidia.com/cuda-downloads). The build script will automatically detect `nvcc` and enable the GPU features.
3. Build the application:
   ```powershell
   # If you have CUDA installed, you can use the build script:
   ./scripts/build.ps1
   # Or run manually:
   cargo run --release
   ```

### Linux (Wayland)
Wayland has a strict security model. DPAIASR implements native Wayland protocols to provide global dictation securely. 

**Required Dependencies:**
1. **GTK4 Development Headers**: Required to compile the floating UI overlay.
2. **`wtype`**: A CLI utility that communicates with the `wlr-virtual-keyboard` protocol for text injection.

#### Ubuntu / Debian
```bash
sudo apt update
sudo apt install libgtk-4-dev build-essential pkg-config
sudo apt install wtype
```

#### Fedora
```bash
sudo dnf install gtk4-devel
sudo dnf install wtype
```

#### Arch Linux
```bash
sudo pacman -S gtk4 pkgconf
sudo pacman -S wtype
```

**Build & Run:**
```bash
cargo run --release
```

---

## 🛑 Wayland Troubleshooting

Due to the fragmented nature of Wayland compositors, some desktop environments may require extra configuration.

- **Sway & Hyprland (wlroots)**: 
  Native support. `gtk4-layer-shell` and `wtype` work out of the box.
- **GNOME**: 
  GNOME disables layer-shell and virtual keyboard protocols by default. To get the UI overlay and text injection working, you must install third-party GNOME extensions that enable `wlr-layer-shell` and `wlr-virtual-keyboard`. Otherwise, DPAIASR will gracefully fall back to holding transcribed text in the clipboard (`Alt+C`).
- **Global Hotkeys Not Working?**: 
  If the `global-hotkey` library fails to register `Alt+M` via your DE's XDG Desktop Portal, simply bind your compositor's native shortcut to run `dpaiasr --toggle`.

---

## 📄 License
This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
