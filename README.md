# Un

The open engine behind Unnumberium: a camera looks at the table, recognises and counts the
objects on it and the child's fingers, and a tutor talks about what it sees, listens to the
spoken answer and adapts. One Rust core on **macOS, Windows, Linux, iOS and Android**;
everything runs on the device and nothing leaves it.

The engine is deliberately empty of lessons. What the tutor asks, how difficulty moves and
what it says is a *strategy* (`un_core::strategy::Strategy`) plugged in from another crate.
This repository ships one tiny demo strategy; the real methodics live in a separate,
proprietary repository and plug in through the same trait. The engine is LGPL-3.0-or-later,
so it can be reused in products that keep their lessons closed, as long as the engine itself
stays open and relinkable (see [Licence](#licence)).

## Layout

| Path | What |
|---|---|
| `crates/un-core` | Pure, platform-independent: geometry, detections, hand joints and finger counting, the debounced table state and object numbering, languages and numerals, the forgiving number parser, and the **strategy interface**. Fully unit-tested. |
| `crates/un-engine` | The lesson runtime: runs a strategy against perception, speech and taps; owns timers, listening, the finger watcher and the HUD snapshot. Speech is behind traits. A listen in `ListenMode::Counting` collects the numbers of a count said aloud across the child's pauses (every recogniser cuts an utterance at a pause), reporting each utterance as `Event::Counted` and the whole sequence as `Event::Numbers` once the child has been quiet for five seconds after the last number or the tutor's last word, or for a beat and a half after the sequence reached the expected answer. |
| `crates/un-vision` | ONNX Runtime (`ort`): object detector (Ultralytics raw heads, end-to-end rows, RT-DETR rows) and MediaPipe-style hand landmarks (palm detection + 21 landmarks, converted to ONNX). |
| `crates/un-speech` | sherpa-onnx: an always-on microphone (Silero VAD, Whisper or the small Zipformer recognisers) so an answer is heard the moment the tutor stops talking, Piper voices for the tutor, an optional "listen now" chime, and a round-trip self-test. |
| `crates/un-media` | Desktop camera capture (AVFoundation, Media Foundation, V4L2) and image files. |
| `crates/un-app` | Assembles a session: finds models, builds speech in/out, runs perception, simulated table. |
| `crates/un-cli` | `un`: headless counting and lessons, the speech self-test, camera and strategy listing. |
| `crates/un-ffi` | UniFFI bindings for Swift and Kotlin: sessions, snapshots, the shared vision, and callback interfaces for platform speech. |
| `apps/desktop` | The desktop window (egui): camera view with overlay, tutor HUD, number pad. A library plus a binary that ships the demo lesson. |
| `apps/ios`, `apps/android` | SwiftUI and Compose shells over `un-ffi`: the phone's camera and speech APIs, the shared engine. The iOS screen is the reference: the tutor's line and status, the camera picture with the overlay (objects, numbers, the finger's ring, fingers, hand skeletons), the number pad, start/end, repeat, skip and the EN/RU switch; Android and the desktop's live view carry the same controls. |
| `strategies/demo` | The smallest strategy: "how many are on the table?" |
| `tools/` | `fetch_models.py` (hand, speech models), `export_detector.py` (export a detector), `teach.py` (fine-tune on `un collect` frames). |
| `docs/architecture.md` | Design, the strategy boundary, and the per-platform feasibility matrix. |

## Setup

Rust 1.92+ (`rustup`), Python 3 for the model tools. On Linux: `libasound2-dev libudev-dev libxkbcommon-dev libwayland-dev libgtk-3-dev`.

```sh
python3 tools/fetch_models.py --speech     # hand models (15 MB) + EN/RU recognisers, VAD and voices (~400 MB)
cargo build --release --workspace
cargo run --release -p un-cli -- check-speech --language ru   # the voice says "три", the recogniser hears it
```

The Zipformers are small and quick but mishear short answers ("ten caps" became "and cabs").
`python3 tools/fetch_models.py --whisper small` adds a multilingual Whisper (about 250 MB) that
the engine then prefers for every language; it decodes an answer in about half a second on an
Apple laptop. `un listen` prints what the microphone hears, for checking a setup.

The object detector is not downloaded: it is trained or exported for the objects you count.
`tools/export_detector.py` produces `models/detector.onnx` plus its `.json` sidecar from an
Ultralytics model (zero-shot YOLOE prompts or your own weights) or any detector whose output
matches one of the supported heads. Note the licence of the weights you ship (Ultralytics'
are AGPL-3.0; RT-DETR, D-FINE and YOLOX are Apache-2.0).

The zero-shot prompts cover cubes, caps, circles, coins, pencils, toy cars, balls, fruit and
stones. For new objects, or for a detector that is sure of *your* objects on *your* table,
teach it: record auto-labelled frames with the CLI, then fine-tune and export.

Two labels are special: labels starting with `_` are decoys (recognised so that a keyboard or a
hand never becomes an object, then dropped), and `finger` is shown over the picture but never
counted, so a child can point at an object. The caps detector carries it:

```sh
python3 tools/export_detector.py --name caps --class "cap=bottle cap,red bottle cap,plastic bottle cap,coca cola cap" \
    --class "finger=finger,index finger,fingertip,pointing finger" \
    --class "_keyboard=laptop keyboard,computer keyboard,trackpad" --class "_hand=hand"
```

```sh
python3 tools/export_detector.py --name stones --class "stone=stone,pebble,rock" --class "_hand=hand"
cargo run --release -p un-cli -- collect --camera 0 --model stones --conf 0.15 --labels stone --out datasets/stones
uv run --with ultralytics,onnx,onnxslim python3 tools/teach.py --dataset datasets/stones --name table
```

Move the objects between grabs, add and remove some, cover a few with a hand. The result,
`models/table.onnx`, is what the desktop app picks by default when it exists.

## Run

```sh
cargo run --release -p un-desktop                 # the window: camera or a simulated table, demo lesson
cargo run --release -p un-cli -- cameras
cargo run --release -p un-cli -- run --camera 0 --model coco     # one JSON line per state change
cargo run --release -p un-cli -- lesson --sim 3 --answers 3,2 --voice console   # a scripted lesson, no camera (words work too: Mia,yes,3)
cargo run --release -p un-cli -- lesson --camera 0 --language ru                # the real thing, headless
```

Desktop keys: drag a rectangle over the table area, `r` resets it, digits answer on the number
pad, `space` repeats the prompt, `n` skips a task, `+`/`-` change the simulated table.

sherpa-onnx is linked as shared libraries (`libsherpa-onnx-c-api`, `libonnxruntime` next to the
executable; cargo puts them in `target/<profile>/deps`, where the binaries look too). Ship them
beside the executable.

## Plugging in a strategy

```rust
use std::sync::Arc;
use un_core::strategy::{Effect, Event, LinkedStrategy, SessionConfig, Strategy, StrategyFactory, StrategyInfo, StrategySnapshot};

struct MyLesson { /* ... */ }
impl Strategy for MyLesson {
    fn handle(&mut self, event: Event) -> Vec<Effect> { /* the reducer */ vec![] }
    fn snapshot(&self) -> StrategySnapshot { /* for the HUD */ todo!() }
}
struct MyFactory;
impl StrategyFactory for MyFactory {
    fn info(&self) -> StrategyInfo { todo!() }
    fn create(&self, config: &SessionConfig) -> Box<dyn Strategy> { Box::new(MyLesson { /* ... */ }) }
}
inventory::submit! { LinkedStrategy(|| Arc::new(MyFactory)) }   // found by every binary that links this crate
```

A product is a binary (or a mobile library) that links the engine and its strategy crates:
`un_desktop::run(registry)`, `un_cli::main()`, or `un-ffi` re-exported from a `staticlib`. The
`unmethod` repository does exactly this for the Unnumberium lessons.

## Platforms

| | Camera | Detector + hands | Speech in | Speech out | UI |
|---|---|---|---|---|---|
| macOS | nokhwa (AVFoundation) | `un-vision` (ORT CPU/CoreML) | sherpa-onnx | sherpa-onnx (Piper) | egui |
| Windows | nokhwa (Media Foundation) | `un-vision` (ORT CPU/DirectML) | sherpa-onnx | sherpa-onnx | egui |
| Linux | nokhwa (V4L2) | `un-vision` (ORT CPU) | sherpa-onnx | sherpa-onnx | egui |
| iOS | AVFoundation (Swift) | `un-vision` via FFI, or Vision hand pose | Apple Speech or sherpa-onnx Swift | AVSpeechSynthesizer or Piper | SwiftUI |
| Android | CameraX (Kotlin) | `un-vision` via FFI (ORT CPU/NNAPI), or MediaPipe | Android SpeechRecognizer or sherpa-onnx Kotlin | Android TTS or Piper | Compose |

See `docs/architecture.md` for the reasoning and the state of each shell.

## Tests

```sh
cargo test --workspace            # ~100 unit tests: state machines, parsers, decoders, the runtime end to end
cargo run -p un-cli -- check-speech
```

## Licence

LGPL-3.0-or-later (`LICENSE`, with the GPL text it refers to in `LICENSE.GPL`). Third-party
components: ONNX Runtime, `ort`, egui, nokhwa, cpal (MIT/Apache-2.0); sherpa-onnx, the
Zipformer models and MediaPipe's hand models (Apache-2.0); Whisper, Silero VAD and Piper voices (MIT).
