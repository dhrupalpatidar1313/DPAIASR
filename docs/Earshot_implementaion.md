Yes. I looked up the **current Earshot Rust crate and its implementation details**, rather than relying only on the earlier discussion. There is a lot that makes it a strong fit for your project, but there are also a few things I would specifically validate before making it your production VAD.

## 1. What Earshot actually is

**Earshot is a lightweight neural Voice Activity Detector implemented in pure Rust.**

The current crate is `earshot 1.2.1`. Its API exposes a `Detector` and a `DefaultPredictor`; the detector maintains state for a single audio stream. ([Docs.rs][1])

Conceptually:

```text
Microphone
    ↓
16 kHz mono audio
    ↓
256-sample frame
    ↓
Earshot Detector
    ↓
speech probability 0.0 → 1.0
    ↓
Your endpointing logic
    ↓
utterance
```

This is important:

**Earshot is a VAD detector, not your complete utterance/session manager.**

It tells you something like:

```text
0.03 → probably silence
0.12 → probably silence
0.81 → speech
0.94 → speech
0.88 → speech
0.06 → silence
```

Then **your own code** decides:

> “The speech has ended after enough consecutive low-score frames, so this utterance is complete.”

That's exactly the architecture you need.

---

# 2. Why Earshot is interesting for your project

The biggest advantage is that it is designed as a **very small native Rust component**.

The current project documentation reports:

* about **8 KiB of state per `Detector`**
* roughly **110 KiB binary footprint**
* around **75 KiB for the neural network**
* `#![no_std]` support
* no ONNX Runtime dependency. ([Docs.rs][2])

That is a very different architectural profile from a typical ONNX-based neural VAD.

For comparison, the Earshot project documentation says Silero's model is around 2 MiB and that using ONNX Runtime adds significantly more binary and memory overhead. Those are project-reported figures, so I would treat them as comparative claims rather than universal benchmarks for your exact build. ([Docs.rs][2])

For your application:

```text
Local
↓
Always available during a session
↓
Runs continuously
↓
Only purpose is speech gating
```

you generally want the VAD to be **cheap**.

You don't want your “cheap gate” consuming a disproportionate amount of your CPU/RAM budget.

---

# 3. It is neural, but much smaller than Silero

This is the interesting technical part.

Earshot's project author describes it as using a **small recurrent neural network**, while Silero is a substantially larger recurrent neural VAD. The Earshot implementation is optimized specifically around low computational cost. ([Reddit][3])

The current source also shows that Earshot performs signal processing before feeding features into its predictor:

```text
audio
 ↓
pre-emphasis
 ↓
FFT / spectral processing
 ↓
mel-like features
 ↓
neural predictor
 ↓
score
```

The current implementation defines a 1024-point FFT, 40 mel features, and contextual frames around the current frame. ([Docs.rs][4])

So this is **not** simply:

```text
RMS > threshold
```

It is a genuine neural speech detector.

---

# 4. Exactly what goes into Earshot

This matters for your pipeline.

The current API expects:

```text
16 kHz
256 samples
```

which means:

```text
256 / 16000
= 0.016 seconds
= 16 ms
```

per frame. The crate also accepts `f32` frames through `predict_f32`, with samples expected in `[-1, 1]`. ([Docs.rs][2])

So your existing preprocessing pipeline fits nicely:

```text
cpal
 ↓
native sample rate
 ↓
mono
 ↓
16 kHz
 ↓
f32 [-1, 1]
 ↓
256 samples
 ↓
Earshot
```

That is a clean integration.

---

# 5. Earshot produces a score, not a final boolean

This is an important distinction.

You get:

```text
score ∈ [0, 1]
```

The documentation says `0.5` is a reasonable starting threshold, but that threshold is adjustable for application-specific behavior. ([Docs.rs][2])

For example:

```text
score >= 0.5
    → speech candidate

score < 0.5
    → non-speech candidate
```

But **don't immediately build your whole endpointing logic around `0.5`**.

Your actual application should have at least:

```text
speech_threshold
silence_threshold
minimum_speech_duration
hangover_duration
minimum_utterance_duration
maximum_utterance_duration
```

That gives you hysteresis rather than a single brittle threshold.

---

# 6. Why hysteresis matters for your project

Suppose the score looks like:

```text
0.80
0.91
0.87
0.53
0.47
0.55
0.48
0.38
```

If you simply do:

```text
score >= 0.5 → speech
```

you can get:

```text
speech
speech
speech
speech
silence
speech
silence
silence
```

That creates ugly utterance fragmentation.

Instead use something like:

```text
Start speech:
    score >= START_THRESHOLD

Stay in speech:
    tolerate temporary dips

End speech:
    score < END_THRESHOLD
    continuously for N ms
```

Conceptually:

```text
START_THRESHOLD = 0.60
END_THRESHOLD   = 0.40
```

These numbers are examples, **not values I am recommending you hard-code**.

You should tune them using your own recordings.

---

# 7. Earshot's detector is stateful

This is another important property.

The detector isn't just:

```text
predict(frame)
```

with every frame completely independent.

It maintains internal state.

The documentation explicitly says:

> create one detector per separate audio stream.

And the current implementation contains internal buffers and predictor state. ([Docs.rs][1])

This fits your architecture perfectly.

You should have:

```text
Audio session
    ↓
one Earshot Detector
    ↓
many 16 ms frames
```

not:

```text
new Detector()
for every frame
```

That would destroy the temporal context the detector relies on.

---

# 8. When should you reset Earshot?

The current documentation explicitly says the detector should be reset when:

* the recording device changes
* the detector is used for a new audio sequence. ([Docs.rs][4])

For your application, I'd interpret that as:

```text
new microphone/device
        → reset

new completely independent capture sequence
        → reset
```

You should **not** reset it after every utterance.

Your model is:

```text
START SESSION
   ↓
Earshot detector stays alive
   ↓
utterance 1
   ↓
utterance 2
   ↓
utterance 3
   ↓
STOP SESSION
```

That is a much better fit.

---

# 9. Your complete Earshot pipeline

I'd implement your Phase 3 like this:

```text
                  Audio Capture
                       │
                       ▼
              ┌────────────────┐
              │ Preprocessor   │
              │ mono / 16k /   │
              │ f32             │
              └───────┬────────┘
                      │
              256 samples / 16ms
                      │
                      ▼
              ┌────────────────┐
              │ Earshot        │
              │ Detector       │
              └───────┬────────┘
                      │
                  score 0..1
                      │
                      ▼
              ┌────────────────┐
              │ Endpoint       │
              │ Manager        │
              │                │
              │ hysteresis     │
              │ hangover       │
              │ min/max time   │
              └───────┬────────┘
                      │
              completed utterance
                      │
                      ▼
                    ASR
```

This is what I would consider the correct conceptual integration.

---

# 10. Earshot does NOT replace your endpointing layer

This is probably the most important thing to understand.

You previously designed:

```text
Silero VAD
  +
200–500 ms hangover
  ↓
utterance completion
```

You should keep this same architectural idea with Earshot.

In other words:

```text
Earshot = speech probability
Endpoint manager = utterance boundary
```

Don't make Earshot responsible for:

* session Start/Stop
* pending queues
* ASR scheduling
* final utterance boundaries
* microphone lifecycle

Those remain application responsibilities.

---

# 11. How Earshot fits your Session model

Your finalized architecture becomes:

```text
USER CLICKS START
        ↓
Session = ACTIVE
        ↓
Audio capture starts
        ↓
Earshot detector continuously evaluates frames
        ↓
Speech begins
        ↓
utterance buffer starts
        ↓
Speech continues
        ↓
Pause
        ↓
Earshot scores fall
        ↓
Endpoint Manager starts hangover
        ↓
Silence persists
        ↓
Utterance COMPLETE
        ↓
Target Snapshot
        ↓
ASR
        ↓
Injection
        ↓
Session continues
```

Then:

```text
USER CLICKS STOP
        ↓
capture stops
        ↓
existing utterance/ASR jobs drain
        ↓
session finishes
```

That is completely compatible with the decisions we've already made.

---

# 12. Performance

This is where Earshot gets very interesting.

The project's own published benchmark claims an RTF around **0.0007**, and describes it as roughly **20× faster than the compared Silero VAD v6 and TEN VAD measurements**. ([Docs.rs][2])

The maintainer later reported an optimized figure around **4.4 µs per 16 ms frame**, which is an extremely small amount of compute relative to the 16 ms of audio represented by the frame. That's a project benchmark, not a guarantee for your i5-9400F build. ([Reddit][5])

The older published announcement reported a little over 10 µs per 16 ms frame. The change in reported numbers is itself a reason to benchmark the exact version/configuration you ship rather than taking one number as definitive. ([Reddit][3])

The important practical point is:

**Earshot's expected computational cost is tiny compared with Whisper inference.**

Your expensive component will remain:

```text
Whisper
```

not:

```text
Earshot
```

---

# 13. Memory footprint

The current documentation says approximately:

```text
Detector state ≈ 8 KiB
Binary ≈ 110 KiB
NN ≈ 75 KiB
```

with the exact binary footprint depending on build/configuration. ([Docs.rs][2])

That's tiny compared with your planned Whisper model.

For your application, the rough architecture becomes:

```text
Earshot
    ~tiny

Overlay / platform runtime
    ~small

Whisper
    ~major memory consumer

Audio buffers
    ~small
```

This is exactly the profile you want.

---

# 14. Does Earshot require ONNX Runtime?

**No.**

This is one of its biggest advantages for you.

Earshot is a native Rust implementation and does not need an external ONNX Runtime just to execute the VAD model. The crate's own source and package metadata show only a very small dependency surface; the optional `libm` dependency is used for the `no_std` configuration. ([Docs.rs][6])

Compare that with the Silero architecture you're currently considering, where an ONNX session is normally part of the runtime. A current Rust Silero wrapper explicitly describes an ONNX Runtime-based session and stateful recurrent stream state. ([Docs.rs][7])

For your project:

```text
Earshot
    ↓
pure Rust

Silero
    ↓
Rust
    ↓
ONNX runtime
    ↓
model
```

The first is substantially simpler operationally.

---

# 15. Does it work on Windows and Ubuntu?

Because it is **pure Rust**, Earshot itself is not tied to Windows APIs.

The docs.rs package currently shows a Linux build target and the crate's source has no Windows-specific VAD dependency. ([Docs.rs][8])

So from an architectural perspective:

```text
Windows
   ↓
cpal
   ↓
Earshot

Ubuntu
   ↓
cpal
   ↓
Earshot
```

is exactly what you want.

The platform-specific work in your project remains around:

```text
overlay
focus
global hotkeys
text injection
tray
autostart
session/desktop integration
```

not VAD.

---

# 16. What about `no_std`?

Earshot supports `#![no_std]`, although your desktop application obviously does **not need** that.

The interesting point is what `no_std` tells you about the library's design:

> It has intentionally been built to keep the runtime footprint very small.

For your desktop app, just use the normal `std` feature.

The crate's current feature configuration is essentially:

```text
default = std
libm   = optional
alloc  = available
```

and `no_std` is enabled by disabling default features and enabling `libm`. ([Docs.rs][6])

---

# 17. What is the underlying neural architecture?

The maintainer describes Earshot as a smaller recurrent neural network using **minGRU**, with training/optimization work including a technique called **THORN**. ([Reddit][3])

You don't need to understand the training details to integrate it.

From your application's perspective:

```text
audio frame
   ↓
feature extraction
   ↓
small recurrent model
   ↓
voice probability
```

The important engineering property is that the model is small enough to run extremely cheaply.

---

# 18. What accuracy should you expect?

This is where I want to be very precise.

The Earshot project claims it is both faster and more accurate than the compared VADs, including Silero v6 and TEN VAD. ([Docs.rs][2])

But **I would not translate that into**:

> “Earshot is definitely more accurate than Silero for my dictation environment.”

That's not established by the information we have.

Your real environment is:

```text
microphone
+
room acoustics
+
fan
+
keyboard
+
speech style
+
accent
+
distance from mic
+
possibly TV/music
```

VAD performance is highly environment-dependent.

So for your product, the correct rule is:

> **Treat Earshot's published benchmark as evidence that it is worth testing, not as the final reason to select it.**

---

# 19. The test I would require before locking Earshot

Create a real recording set from your own environment.

### Clean

```text
"Create a function that validates the token."
```

### Short pauses

```text
"Create a function..."

[pause 300 ms]

"...that validates the token."
```

### Long pauses

```text
"Create a function."

[pause 2 seconds]

"It should return false when the token expires."
```

### Background noise

```text
fan
air conditioner
keyboard
mouse clicks
```

### Competing speech

```text
TV
YouTube
another person talking
```

### Continuous speech

```text
30–60 seconds without deliberate pauses
```

Then compare:

```text
Earshot
Silero
Energy/RMS
```

using identical recordings.

---

# 20. What metrics matter for your project

Don't only measure accuracy.

Measure:

### False starts

```text
noise → Earshot says speech
```

### Missed speech

```text
speech → Earshot says silence
```

### Endpoint delay

```text
actual speech end
        ↓
utterance declared finished
```

### Speech clipping

```text
How many ms of speech were lost at the start/end?
```

### Fragmentation

```text
One sentence
    ↓
2 utterances
```

That's bad.

### Merge rate

```text
Two sentences separated by a pause
    ↓
1 utterance
```

That may also be bad for your ASR latency.

### CPU

Measure the actual process CPU on your:

```text
i5-9400F
```

### Memory

Measure resident memory during:

```text
idle
listening
VAD
ASR
```

---

# 21. One thing I especially like about Earshot for your design

Your original goal was:

> **VAD-gated local ASR to keep background CPU usage low.**

Earshot is extremely aligned with that.

You want:

```text
Session active
      ↓
VAD always running
      ↓
Whisper only runs when utterance ends
```

The VAD is therefore running much more frequently than the ASR.

That means VAD efficiency matters.

Conceptually:

```text
10 minutes of session
│
├── Earshot: thousands of tiny frame evaluations
│
└── Whisper: perhaps dozens of utterances
```

So a lightweight VAD is a very sensible optimization target.

---

# 22. But don't over-optimize the wrong thing

There is an important performance perspective.

Suppose:

```text
Earshot = 0.01% CPU
Whisper = 40% CPU during inference
```

Trying to save another 50% of Earshot's already tiny CPU usage doesn't materially improve your application.

So the actual priorities should be:

```text
1. VAD accuracy
2. Endpoint quality
3. False-trigger rate
4. ASR latency
5. VAD CPU/memory
```

Earshot appears strong on #5.

You still need to validate #1–#3 in your environment.

---

# 23. How I would implement the VAD abstraction

Keep your earlier interface idea.

Conceptually:

```text id="5hk6v7"
trait VadEngine {
    fn process_frame(&mut self, frame: &[f32]) -> VadScore;
    fn reset(&mut self);
}
```

Then:

```text id="7q23yx"
EarshotVad
EnergyVad
SileroVad
```

But I would **not ship all three**.

Use:

```text
EnergyVad
    ↓
development/testing

EarshotVad
    ↓
production candidate
```

and keep Silero available as the benchmark/reference implementation.

---

# 24. Your Earshot implementation should probably NOT have a giant API

You don't need:

```text
start()
stop()
flush()
transcribe()
detect()
segment()
session()
...
```

inside the VAD.

Keep it narrow:

```text id="f7f5a4"
audio frame
    ↓
score
```

Then your `EndpointManager` handles:

```text
score
 ↓
speech state
 ↓
hangover
 ↓
utterance boundaries
```

And `SessionController` handles:

```text
START
STOP
```

This separation will keep your architecture clean.

---

# 25. One subtle issue: frame size

Your original architecture talked about 30 ms VAD windows. 

Earshot's current API expects **256 samples = 16 ms at 16 kHz**. ([Docs.rs][2])

Therefore, if you switch to Earshot, I would **change the VAD frame contract from 30 ms to 16 ms**.

This isn't a problem.

Your pipeline can simply be:

```text
16 kHz
 ↓
256 sample frames
 ↓
Earshot
```

Then your endpoint manager can still use something like:

```text
300 ms hangover
```

because those are separate concepts.

This is an important architecture correction.

---

# 26. Earshot vs Silero for your project

| Factor                    | Earshot        | Silero                                          |
| ------------------------- | -------------- | ----------------------------------------------- |
| Neural VAD                | Yes            | Yes                                             |
| Pure Rust                 | **Yes**        | Wrapper/runtime dependent                       |
| ONNX Runtime              | **No**         | Typically yes                                   |
| Memory footprint          | **Very small** | Larger                                          |
| CPU cost                  | **Very low**   | Higher                                          |
| Frame size                | 16 ms          | 8/16 kHz model chunks; implementation-dependent |
| Cross-platform core       | **Excellent**  | Good                                            |
| Rust integration          | **Excellent**  | More infrastructure                             |
| `no_std`                  | **Yes**        | No                                              |
| Published accuracy claims | Strong         | Strong/mature                                   |
| Ecosystem maturity        | Newer          | More established                                |
| Your project fit          | **Excellent**  | Very good                                       |

The current Rust Silero wrapper documents a stateful ONNX session with per-stream state and 8/16 kHz input chunks, which demonstrates the extra runtime structure involved. ([Docs.rs][7])

---

# 27. Is Earshot ready for your project?

My answer:

**Technically, yes as a serious candidate.**

But I would phrase the decision as:

> **Earshot is the production VAD candidate; Silero remains the validation benchmark until our real-world tests prove Earshot meets the required false-trigger, endpointing, and speech-retention targets.**

That is stronger engineering than blindly declaring:

> “Earshot is better.”

---

# 28. What I would change in your implementation roadmap

Instead of:

```text
Phase 3 → Silero VAD
```

make it:

```text
Phase 3A → VAD abstraction + Energy baseline
Phase 3B → Earshot integration
Phase 3C → Real-world VAD benchmark
Phase 3D → Lock production VAD
```

And the acceptance criterion should be something like:

```text
Earshot passes:
✓ clean speech
✓ natural pauses
✓ noisy environment
✓ keyboard noise
✓ competing speech
✓ long speech
✓ low false-trigger rate
✓ acceptable endpoint latency
✓ acceptable CPU
✓ Windows
✓ Ubuntu
```

Only then do you lock it.

### Bottom line

For **your specific architecture**, Earshot is compelling because it gives you a **small, native-Rust, neural VAD with no ONNX Runtime**, while matching your 16 kHz local-CPU pipeline well. Its current crate is actively maintained and has a very small runtime footprint. ([Docs.rs][1])

I would therefore move your plan from **“Silero is the production VAD”** to **“Earshot is the primary candidate, Silero is the accuracy/reference benchmark, and the final choice is made from your real recordings.”**

[1]: https://docs.rs/earshot/latest/earshot/?utm_source=chatgpt.com "earshot - Rust"
[2]: https://docs.rs/crate/earshot/latest/source/README.md?utm_source=chatgpt.com "earshot 1.1.0 - Docs.rs"
[3]: https://www.reddit.com/r/rust/comments/1skh3bb/earshot_a_ridiculously_fast_voice_activity/?utm_source=chatgpt.com "Earshot, a ridiculously fast voice activity detection crate"
[4]: https://docs.rs/crate/earshot/latest/source/src/lib.rs?utm_source=chatgpt.com "earshot 1.2.1 - Docs.rs"
[5]: https://www.reddit.com/r/rust/comments/1v1bztc/speeding_up_voice_activity_detection_by_going/?utm_source=chatgpt.com "Speeding up voice activity detection by going back to the 90s"
[6]: https://docs.rs/crate/earshot/latest/source/Cargo.toml?utm_source=chatgpt.com "earshot 1.2.1 - Docs.rs"
[7]: https://docs.rs/silero/latest/silero/?utm_source=chatgpt.com "silero - Rust"
[8]: https://docs.rs/earshot/latest/earshot/ "earshot - Rust"
