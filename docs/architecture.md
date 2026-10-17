# Architecture

## One core, thin shells

```
          ┌────────────────────────────── strategy crates (any licence) ──────────────────────────────┐
          │  un-strategy-demo (here)        unmethod-counting (private)        your-lesson            │
          └──────────────────────────────────────────┬────────────────────────────────────────────────┘
                                                     │ Strategy trait + link-time registration
┌───────────────────────────────────────────── un (LGPL) ─────────────────────────────────────────────┐
│ un-core ─ geometry, estimator, tracker, language, numbers, strategy interface (pure)                 │
│ un-engine ─ effect queue, timers, listening, snapshot ─ SpeechInput/Output traits                    │
│ un-vision (ort) ─ detector                      un-speech (sherpa-onnx) ─ ASR, VAD, TTS, chime     │
│ un-app ─ model paths, speech setup, perception pipeline, simulated table, session builder           │
│ un-media (desktop camera)   un-cli   apps/desktop (egui)   un-ffi (UniFFI) → apps/ios, apps/android │
└──────────────────────────────────────────────────────────────────────────────────────────────────────┘
```

Frames are plain RGB8 (or NV12 from phones). Every platform produces one; the models and the
state machines behind them are identical everywhere, so a count means the
same thing on a laptop and on a phone.

## The strategy boundary

`un_core::strategy` is the contract: a strategy is a reducer `handle(Event) -> Vec<Effect>`
with a `snapshot()` for the HUD. Events are `Start`, `Stop`, `Table(observation)`, `Heard(n)`,
`Said(words)` (an utterance that is not a number: a name, "yes", "I don't understand"; the
listen goes on), `NothingHeard`, `Sound(bool)` (the microphone picks up sound or has gone quiet,
only with a metered microphone), `TimerFired`, `RepeatRequested`, `SkipRequested`. Effects are
`Say`, `Listen`, `StopListening`, `HighlightCount`, `Celebrate`, `StartTimer`, `CancelTimer`,
`SessionEnded`. The engine never sees a task or a phrase; the strategy never sees a pixel or a
sample.

A session's summary carries `remember`, a string map the strategy wants back next time as
`SessionConfig::extra` (the child's name, where a paused lesson stopped); products store it
(the desktop app in `desktop.json`) and `paused` tells them the lesson can be resumed.

The always-on microphone hears the tutor through the speakers. The engine tells the speech
input when the tutor starts and stops talking (`tutor_speaking`, `tutor_silent`); an utterance
overlapping that window is kept only if it still holds a number once the tutor's own number
words are removed, otherwise it is dropped as echo.

Strategies register at link time (`inventory`), so a product is just a binary that links the
engine and the strategy crates it has the rights to. The engine stays LGPL; the strategies may
be anything. Because the engine crates are statically linked into Rust binaries, a closed
product must satisfy LGPL §4 by letting users relink against a modified engine: ship object
files or a dynamic `un-ffi` library, and keep the engine's source and build instructions
available. (A product whose author owns both sides, like Unnumberium, has no obligation to
itself; this matters for third parties reusing the engine.)

## Perception

* **Detector**: an exported ONNX model, letterboxed to the model input, decoded from the raw head
  (Ultralytics one-to-many `[1, 4+nc, anchors]`, end-to-end rows `[1, K, 6]`, RT-DETR rows
  `[1, K, 4+nc]`), class-agnostic NMS, decoys (labels starting with `_`) dropped.
* **Table state**: lower-median windows that commit after 0.7 s; left/right
  zones; overlap and midline ambiguity flags. Objects get stable 1…n numbers from a greedy tracker.

## Speech

* **Desktop**: sherpa-onnx everywhere. Whisper when installed, else Zipformer transducers (EN small, RU int8), cut into utterances by
  Silero VAD, with automatic gain for quiet headsets; Piper voices through sherpa-onnx's VITS runtime
  (no Python, no espeak binary). `un check-speech` runs the voice into the recogniser with no audio device.
* **Phones**: the shell implements `SpeechInputCallback`/`SpeechOutputCallback` over the platform's
  recogniser and synthesiser, or over sherpa-onnx's Swift/Kotlin packages for the same voices as desktop.

## Platform feasibility

| Platform | Status | Notes |
|---|---|---|
| macOS | runs | Verified: camera 25 fps with the detector (M-series, CPU), speech self-test EN/RU. |
| Windows | builds (cross-compiled with cargo-xwin) | nokhwa Media Foundation, cpal WASAPI, sherpa-onnx prebuilt; DirectML via the `directml` feature. |
| Linux | builds | V4L2, ALSA; needs the dev packages listed in the README. |
| iOS | shell scaffolded | `apps/ios`: AVFoundation camera → `UnPerception` (NV12), Apple speech adapters, SwiftUI screen. Needs `cargo build --target aarch64-apple-ios`, bindings generation and an Xcode run to validate. |
| Android | shell scaffolded | `apps/android`: CameraX → `UnPerception`, Android TTS/SpeechRecognizer adapters, Compose screen. Needs the NDK (`cargo-ndk`) and a device run to validate. |

What is not done: signed installers; Russian "how many?" questions outside a lesson; an
on-device model updater; a child-friendly visual design beyond the developer HUD.
