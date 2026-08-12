# Project Structure

This structure follows the platform-independent architecture. Shared pipeline
code is separate from future operating-system adapters.

```text
DPAIASR/
├── docs/
│   ├── project_structure.md
│   ├── voice_dictation_architecture_V0.0.md
│   └── voice_dictation_architecture_V1.md
├── src/
│   ├── lib.rs                # Shared core facade and public API
│   ├── main.rs               # Thin executable entry point
│   ├── audio/
│   │   ├── capture.rs        # T1 microphone capture via cpal
│   │   └── preprocess.rs     # PCM normalization, resampling, and framing
│   ├── vad/
│   │   ├── trait.rs          # Shared VAD interface, state, events, config
│   │   └── energy.rs         # Development/test VAD implementation
│   ├── core/
│   │   └── session.rs        # Shared session state vocabulary
│   ├── platform/
│   │   └── mod.rs            # Platform boundary; no native handles in core
│   ├── asr/                  # T3 local speech recognition worker
│   ├── transcript/            # Minimal transcript cleanup
│   ├── targeting/            # Platform-neutral target policy interfaces
│   ├── injection/            # Platform-neutral injection interface
│   ├── pending/              # Shared bounded FIFO pending queue
│   └── ui/                   # Platform-neutral UI interfaces
├── tests/                    # Integration tests
└── assets/
    └── models/               # Local ASR model files; do not commit models
```

The audio, preprocessing, VAD, session, and future ASR pipeline are shared.
Windows and Linux-specific behavior belongs under `src/platform/`; Linux must
later distinguish X11 and Wayland compatibility. This remains a single Cargo
package for now.
