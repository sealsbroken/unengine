# Architecture

## One core, thin shells

```
          ┌────────────────────────────── strategy crates (any licence) ──────────────────────────────┐
          │  un-strategy-demo (here)        unmethod-counting (private)        your-lesson            │
          └──────────────────────────────────────────┬────────────────────────────────────────────────┘
                                                     │ Strategy trait + link-time registration
┌───────────────────────────────────────────── un (LGPL) ─────────────────────────────────────────────┐
│ un-core ─ geometry, hands, estimator, tracker, language, numbers, strategy interface (pure)          │
│ un-engine ─ effect queue, timers, listening, finger watcher, snapshot ─ SpeechInput/Output traits    │
│ un-vision (ort) ─ detector, hand landmarks      un-speech (sherpa-onnx) ─ ASR, VAD, TTS, chime     │
│ un-app ─ model paths, speech setup, perception pipeline, simulated table, session builder           │
│ un-media (desktop camera)   un-cli   apps/desktop (egui)   un-ffi (UniFFI) → apps/ios, apps/android │
└──────────────────────────────────────────────────────────────────────────────────────────────────────┘
```

Frames are plain RGB8 (or NV12 from phones). Every platform produces one; the models and the
state machines behind them are identical everywhere, so a count or a finger total means the
same thing on a laptop and on a phone.

## The strategy boundary

`un_core::strategy` is the contract: a strategy is a reducer `handle(Event) -> Vec<Effect>`
with a `snapshot()` for the HUD. Events are `Start`, `Stop`, `Table(observation)`, `Heard(n)`,
`NothingHeard`, `TimerFired`, `RepeatRequested`, `SkipRequested`. Effects are `Say`, `Listen`,
`StopListening`, `HighlightCount`, `Celebrate`, `StartTimer`, `CancelTimer`, `SessionEnded`.
The engine never sees a task or a phrase; the strategy never sees a pixel or a sample.

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
* **Hands**: PINTO0309's ONNX conversion of MediaPipe Hands. Palm detection (192×192, post-processing
  inside the graph) yields a rotated square region; the landmark model (224×224, batched) yields 21
  joints, a presence score and handedness. Regions are carried from frame to frame from the previous
  landmarks, with palm detection re-run periodically or when a hand is lost. Finger counting is the
  geometric heuristic from the iOS app, so Vision (iOS) or MediaPipe Tasks (Android) can feed the same
  `HandObservation` if a platform prefers its native tracker.
* **Table state**: lower-median windows that commit after 0.7 s with no hand over the table; left/right
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
| macOS | runs | Verified: camera 25 fps with COCO detector + hands (M-series, CPU), speech self-test EN/RU. |
| Windows | builds in CI | nokhwa Media Foundation, cpal WASAPI, sherpa-onnx prebuilt; DirectML via the `directml` feature. |
| Linux | builds in CI | V4L2, ALSA; needs the dev packages listed in the README. |
| iOS | shell scaffolded | `apps/ios`: AVFoundation camera → `UnPerception` (NV12), Apple speech adapters, SwiftUI screen. Needs `cargo build --target aarch64-apple-ios`, bindings generation and an Xcode run to validate. |
| Android | shell scaffolded | `apps/android`: CameraX → `UnPerception`, Android TTS/SpeechRecognizer adapters, Compose screen. Needs the NDK (`cargo-ndk`) and a device run to validate. |

What is not done: signed installers; Russian "how many?" questions outside a lesson; an
on-device model updater; a child-friendly visual design beyond the developer HUD.
