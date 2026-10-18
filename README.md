# Un

The open engine behind Unnumberium: a camera looks at the table, recognises and counts the
toys on it, and a tutor talks about what it sees, listens to the
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
| `crates/un-core` | Pure, platform-independent: geometry, detections, the debounced table state and object numbering, languages and numerals, the forgiving number parser, and the **strategy interface**. Fully unit-tested. |
| `crates/un-engine` | The lesson runtime: runs a strategy against perception, speech and taps; owns timers, listening and the HUD snapshot. Speech is behind traits. A listen in `ListenMode::Counting` collects the numbers of a count said aloud across the child's pauses (every recogniser cuts an utterance at a pause), reporting each utterance as `Event::Counted` and the whole sequence as `Event::Numbers` once the child has been quiet for five seconds after the last number or the tutor's last word, or for a beat and a half after the sequence reached the expected answer. |
| `crates/un-vision` | ONNX Runtime (`ort`): object detector (Ultralytics raw heads, end-to-end rows, RT-DETR rows). |
| `crates/un-media` | Desktop camera capture (AVFoundation, Media Foundation, V4L2) and image files. |
| `crates/un-app` | Assembles a session: finds models, builds speech in/out, runs perception, simulated table. |
| `crates/un-cli` | `un`: headless counting and lessons, the speech self-test, camera and strategy listing. |
| `crates/un-ffi` | UniFFI bindings for Swift and Kotlin: sessions, snapshots, the shared vision, and callback interfaces for platform speech. |
| `apps/desktop` | The desktop window (egui): camera view with overlay, tutor HUD, number pad. A library plus a binary that ships the demo lesson. |
| `apps/ios`, `apps/android` | SwiftUI and Compose shells over `un-ffi`: the phone's camera and speech APIs, the shared engine. The iOS screen is the reference: the tutor's line and status, the camera picture with the overlay (toys and their numbers), the number pad, start/end, repeat, skip and the EN/RU switch; Android and the desktop's live view carry the same controls. |
| `strategies/demo` | The smallest strategy: "how many are on the table?" |
| `tools/` | `fetch_models.py` (speech models), `export_detector.py` (export a detector), `teach.py` (fine-tune on `un collect` frames). |
| `docs/architecture.md` | Design, the strategy boundary, and the per-platform feasibility matrix. |

## Setup

Rust 1.92+ (`rustup`), Python 3 for the model tools. On Linux: `libasound2-dev libudev-dev libxkbcommon-dev libwayland-dev libgtk-3-dev`.

```sh
python3 tools/fetch_models.py              # EN/RU recognisers, VAD and voices (~400 MB)
cargo build --release --workspace
cargo run --release -p un-cli -- check-speech --language ru   # the voice says "три", the recogniser hears it
```

The Zipformers are small and quick but mishear short answers ("ten cars" can become "and cards").
`python3 tools/fetch_models.py --whisper small` adds a multilingual Whisper (about 250 MB) that
the engine then prefers for every language; it decodes an answer in about half a second on an
Apple laptop. `un listen` prints what the microphone hears, for checking a setup.

The toy detector, `models/toys.onnx`, is an RF-DETR model (Roboflow, Apache-2.0) trained here on
photos labelled by OWLv2 (Google, Apache-2.0). Nothing in that pipeline is Ultralytics, so the
model ships under Apache-2.0 terms and an app built on it can stay closed or go to the App Store.
It is not downloaded: build it with the two tools below. It has two classes: `toy` (every kind of
toy) and `_hand`, a decoy for hands and fingers. Its input is a 576×576 square (phones send
portrait frames, desktop cameras landscape; both are letterboxed).

```sh
# Photos: your own table (un collect, below) and any other toy photos, e.g. Open Images.
uv run --with torch,transformers,pillow python tools/label.py \
    --images datasets/openimages --images datasets/table/images:8 --out datasets/toys --preview 40
uv run --with "rfdetr[train]",onnx,onnxruntime python tools/train_detector.py --dataset datasets/toys --name toys
```

`label.py` asks OWLv2 for every toy and every hand in each photo (prompts per class with
`--class label=prompt,prompt`) and writes a COCO dataset; `DIR:N` repeats a directory N times in
training, so a few dozen frames of your own table weigh as much as thousands of photos from
elsewhere; `--preview` draws the labels for checking by eye. `train_detector.py` fine-tunes RF-DETR
nano from its COCO weights (Apache-2.0; the larger RF-DETR sizes have another licence) and exports
`models/<name>.onnx` with its sidecar: RGB in 0...1, normalisation inside the graph, and one output
of normalised boxes with a sigmoid score per class (the engine's `rtdetr` head).

On iPhone and iPad the detector runs on CoreML in the MLProgram format (`"coreml": "mlprogram"` in
the sidecar), the only one that keeps RF-DETR's attention (GridSample) on the GPU. The first launch
compiles it, about a minute; the compiled model is cached (`UN_CACHE_DIR`, else the temporary
directory) and loads in seconds after that.

For more photos of *your* toys on *your* table, record frames with the CLI and label them as above:

```sh
cargo run --release -p un-cli -- collect --camera 0 --model toys --conf 0.15 --out datasets/table
```

Move the toys between grabs, add and remove some, cover a few with a hand. The desktop app picks
the most recently exported or trained detector, so a new `models/<name>.onnx` takes over.

Labels starting with `_` are decoys (`_hand`, and for older detectors `_keyboard`, `_finger`):
recognised so that a hand never becomes a toy, then dropped. The engine has no hand or finger
tracking: the child answers by voice (or the number pad), and the camera only counts toys.

The previous detector, Ultralytics' zero-shot YOLOE (`tools/export_detector.py`, `tools/teach.py`),
still works with the engine but is AGPL-3.0 (see [Licence](#licence)); use it for experiments only.

## Lessons and the lesson list

A lesson is a strategy crate (`un_core::strategy::StrategyFactory`). The apps list every
lesson the registry holds and start the one picked: the iOS and Android apps open on a lesson
list (title and description from `StrategyInfo`), the desktop app has its **Lesson** box. By
default the registry is every linked strategy plus the demo. A product shows its own list
instead by calling `un_app::set_registry(my_lessons)` (desktop, CLI) or
`un_ffi::set_registry(my_lessons)` (mobile library, from its startup function) once at startup;
`my_lessons` returns a `StrategyRegistry` in list order. See the `unmethod` repository's
`crates/lessons` for a product list.

## Talking to the tutor

With `EngineOptions::always_listen` (on in every app) the microphone is open for the whole
lesson, not only while the strategy waits for an answer: what the child says while an answer
window is open goes to it (numbers, a count), anything else reaches the strategy as
`Event::Said`. The microphone closes while the tutor speaks, so the tutor never hears or
answers itself, except during a count, where the tutor's "good" overlaps the child's next
number: then it stays open and the tutor's own words are taken out of the transcript.

There is no language model: a strategy answers everything the child says from its own script.

## Run

```sh
cargo run --release -p un-desktop                 # the window: camera or a simulated table, demo lesson
cargo run --release -p un-cli -- cameras
cargo run --release -p un-cli -- run --camera 0                  # toys; one JSON line per state change
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

| | Camera | Detector | Speech in | Speech out | UI |
|---|---|---|---|---|---|
| macOS | nokhwa (AVFoundation) | `un-vision` (ORT CPU/CoreML) | sherpa-onnx | sherpa-onnx (Piper) | egui |
| Windows | nokhwa (Media Foundation) | `un-vision` (ORT CPU/DirectML) | sherpa-onnx | sherpa-onnx | egui |
| Linux | nokhwa (V4L2) | `un-vision` (ORT CPU) | sherpa-onnx | sherpa-onnx | egui |
| iOS | AVFoundation (Swift) | `un-vision` via FFI | Apple Speech or sherpa-onnx Swift | AVSpeechSynthesizer or Piper | SwiftUI |
| Android | CameraX (Kotlin) | `un-vision` via FFI (ORT CPU/NNAPI), or MediaPipe | Android SpeechRecognizer or sherpa-onnx Kotlin | Android TTS or Piper | Compose |

See `docs/architecture.md` for the reasoning and the state of each shell.

## Tests

```sh
cargo test --workspace            # ~100 unit tests: state machines, parsers, decoders, the runtime end to end
cargo run -p un-cli -- check-speech
```

## Releases

All from the Actions tab (or `gh workflow run "<name>"`); the tag is the version, as in onym-ios:
leave it blank for the next patch of the latest `v*` tag (`v0.1.9` → version 0.1.9, build 109).

- **Release iOS**: TestFlight (then the External group and Beta App Review once processed), the
  tag and a GitHub Release, and the Windows and macOS alphas of the same tag.
- **Release Desktop**: the tag and a GitHub Release with the Windows x64 and macOS (Apple silicon)
  alphas, no iOS build.
- **Release Windows** / **Release macOS**: build and attach one platform's zip to an existing tag.
- **TestFlight External**: give an uploaded build to the External group by hand.

The app ships from this repository with [unmath](https://github.com/sealsbroken/unmath) only;
the toy detector is the `models-v1` release. Secrets: `ASC_KEY_ID`, `ASC_ISSUER_ID`,
`ASC_KEY_CONTENT` (sealsbroken organisation, App Store Connect API key) for iOS.

## Licence

LGPL-3.0-or-later (`LICENSE`, with the GPL text it refers to in `LICENSE.GPL`). Third-party
components: ONNX Runtime, `ort`, egui, nokhwa, cpal (MIT/Apache-2.0); sherpa-onnx, the
Zipformer models (Apache-2.0); Whisper and Silero VAD (MIT); the Piper voices' training data has its own terms (Irina's, from
RHVoice, is marked "Unknown" in its model card; check before shipping it);

The toy detector (`models/toys.onnx`) is not in this repository. Built with `tools/label.py` and
`tools/train_detector.py` it is Apache-2.0 work: RF-DETR (Roboflow; code and the nano weights
Apache-2.0), labelled by OWLv2 (Google, Apache-2.0) via Hugging Face Transformers (Apache-2.0).
Photos from Open Images carry CC BY 2.0 (images) and CC BY 4.0 (annotations): credit them where the
model is distributed. Models exported or trained with Ultralytics (`tools/export_detector.py`,
`tools/teach.py`, YOLOE) are AGPL-3.0 instead, and a program shipped with one is a combined work
under the AGPL; do not ship them in a closed or App Store product.

The desktop voice is not LGPL-compatible for closed products. The prebuilt sherpa-onnx library
(`sherpa-onnx-c-api`, used by `un-speech` for Piper voices) contains espeak-ng, which is GPL-3.0:
an app shipped with it is a combined work under the GPL, lessons included. The mobile libraries
leave `un-speech` out and use the platform's speech, so they are not affected. A closed desktop
product needs the system voice instead of Piper, or the voice in a separate GPL process.
