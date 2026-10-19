# Un

The open engine behind Unnumberium. A camera looks at the table and counts the toys on it, and a
tutor talks about what it sees, listens to the child's spoken answer and adapts. One Rust core
runs on **macOS, Windows, Linux, iOS and Android**. Everything runs on the device; nothing leaves it.

The engine has no lessons of its own. What the tutor asks, how the difficulty moves and what it
says is a *strategy* (`un_core::strategy::Strategy`) plugged in from another crate. This
repository has one tiny demo strategy; the real lessons live in
[sealsbroken/unmath](https://github.com/sealsbroken/unmath) and plug in through the same trait.
The engine is LGPL-3.0-or-later, so a product can keep its lessons closed as long as the engine
stays open and relinkable (see [Licence](#licence)).

## Layout

| Path | What |
|---|---|
| `crates/un-core` | Pure and platform-independent: geometry, detections, the debounced table state and object numbering, languages and numerals, the forgiving number parser, and the **strategy interface**. Fully unit-tested. |
| `crates/un-engine` | The lesson runtime: runs a strategy against perception, speech and taps, and owns the timers, the listening and the HUD snapshot. Speech sits behind traits. |
| `crates/un-vision` | ONNX Runtime (`ort`): the toy detector (letterboxing, box decoding, NMS). |
| `crates/un-media` | Desktop camera capture (AVFoundation, Media Foundation, V4L2) and image files. |
| `crates/un-app` | Assembles a session: finds the models, builds speech in and out, runs perception, simulates a table. |
| `crates/un-cli` | `un`: headless counting and lessons, the speech self-test, listing cameras and strategies, recording frames. |
| `crates/un-ffi` | UniFFI bindings for Swift and Kotlin: sessions, snapshots, the shared vision, and callback interfaces for platform speech. |
| `apps/desktop` | The desktop window (egui): camera view with overlay, tutor HUD, number pad. A library, plus a binary that ships the demo lesson. |
| `apps/un`, `apps/mobile` | The products: the desktop app and the mobile library, each linking the engine with the unmath lessons (`../unmath`, next to this repository) and nothing else. |
| `apps/ios`, `apps/android` | SwiftUI and Compose apps over `apps/mobile`, using the device's camera and speech. The iOS screen is the reference; Android and the desktop mirror it. `apps/ios` also builds the same SwiftUI app as a native, sandboxed Mac app (the `UnMac` target) for TestFlight and the Mac App Store. |
| `strategies/demo` | The smallest strategy: "how many are on the table?" |
| `tools/` | `fetch_models.py` (speech models), `label.py` and `train_detector.py` (the toy detector), `ios.sh` and `android.sh` (build the mobile library and bindings). |
| `docs/architecture.md` | The design, the strategy boundary, and what each platform needs. |

## Setup

You need Rust 1.92+ (`rustup`) and Python 3 for the model tools. On Linux also install
`libasound2-dev libudev-dev libxkbcommon-dev libwayland-dev libgtk-3-dev`.

```sh
python3 tools/fetch_models.py              # EN/RU recognisers, VAD and voices (~400 MB)
cargo build --release --workspace
cargo run --release -p un-cli -- check-speech --language ru   # the voice says "три", the recogniser hears it
```

The Zipformer recognisers are small and quick but mishear short answers ("ten cars" can become
"and cards"). `python3 tools/fetch_models.py --whisper small` adds a multilingual Whisper
(about 250 MB), which the engine then prefers for every language. It decodes an answer in about
half a second on an Apple laptop. `un listen` prints what the microphone hears, which helps when
checking a setup.

The tutor's voice on the desktop is Piper: LJSpeech for English and Irina for Russian, both adult
women. No free voice sounds like a child, so every app raises the pitch by 1.45. The desktop
synthesises the line 1.45 times slower and plays it 1.45 times faster (`SpeakerOutput::pitch`);
iOS and Android set the same pitch on the system voice.

### The toy detector

`models/toys.onnx` is an RF-DETR nano model (Roboflow, Apache-2.0), trained here on photos
labelled by OWLv2 (Google, Apache-2.0). The release workflows take it from this repository's
`models-v1` release; to build your own:

```sh
# Photos: your own table (un collect, below) and any other toy photos, e.g. Open Images.
uv run --with torch,transformers,pillow python tools/label.py \
    --images datasets/openimages --images datasets/table/images:8 --out datasets/toys --preview 40
uv run --with "rfdetr[train]",onnx,onnxruntime python tools/train_detector.py --dataset datasets/toys --name toys
```

- `label.py` asks OWLv2 for every toy and every hand in each photo and writes a COCO dataset.
  - `--class label=prompt,prompt` sets the prompts for each class.
  - `DIR:N` repeats a directory N times in training, so a few dozen frames of your own table
    weigh as much as thousands of photos from elsewhere.
  - `--preview` draws the labels so you can check them by eye.
- `train_detector.py` fine-tunes RF-DETR nano from its COCO weights. The larger RF-DETR sizes have
  another licence. It exports `models/<name>.onnx` and its `.json` sidecar.
  - Input: RGB in 0…1, with the normalisation inside the graph.
  - Output: normalised boxes, each with a sigmoid score per class (the `rtdetr` head).

The model has two classes. `toy` covers every kind of toy. `_hand` is a decoy: a label that starts
with `_` is recognised so that a hand never becomes a toy, then dropped. Its input is a square
(the size is in the sidecar). Phones send portrait frames and desktop cameras landscape ones;
both are letterboxed. The engine does not track hands or fingers: the child answers by voice or
on the number pad, and the camera only counts toys.

On iPhone and iPad the detector runs on CoreML in the MLProgram format (`"coreml": "mlprogram"` in
the sidecar). It is the only format that keeps RF-DETR's attention (GridSample) on the GPU. The
first launch compiles the model, which takes about a minute. The compiled model is cached in
`UN_CACHE_DIR` (or the temporary directory) and loads in seconds after that.

To add frames of *your* toys on *your* table, record them with the CLI and label them as above:

```sh
cargo run --release -p un-cli -- collect --camera 0 --model toys --conf 0.15 --out datasets/table
```

Move the toys between grabs, add and remove some, and cover a few with a hand. The desktop app
picks the most recently trained detector, so a new `models/<name>.onnx` takes over.

## Lessons and the lesson list

A lesson is a strategy crate (`un_core::strategy::StrategyFactory`). The apps list every lesson
in the registry and start the one picked. The iOS and Android apps open on the lesson list,
taking each title and description from `StrategyInfo`. The desktop app has a **Lesson** box.

By default the registry holds every linked strategy plus the demo. A product shows its own list
instead: it calls `un_app::set_registry(my_lessons)` (desktop, CLI) or
`un_ffi::set_registry(my_lessons)` (mobile library, from its startup function) once at startup.
`my_lessons` returns a `StrategyRegistry` in list order. `apps/un` and `apps/mobile` do this
with the unmath lessons.

## Talking to the tutor

With `EngineOptions::always_listen`, which every app turns on, the microphone stays open for the
whole lesson, not only while the strategy waits for an answer.

- What the child says while an answer window is open goes to that answer (numbers, a count).
- Anything else reaches the strategy as `Event::Said`.
- The microphone closes while the tutor speaks, so the tutor never hears itself. A count is the
  exception: the tutor's "good" overlaps the child's next number, so the microphone stays open
  and the tutor's own words are taken out of the transcript.

A listen in `ListenMode::Counting` collects a count said aloud across the child's pauses (every
recogniser cuts an utterance at a pause). It reports two kinds of event:

- `Event::Counted` for each utterance.
- `Event::Numbers` for the whole sequence. It is sent once the child has been quiet for five
  seconds after the last number or the tutor's last word, or a beat and a half after the
  sequence reaches the expected answer.

There is no language model: a strategy answers everything the child says from its own script.

## Run

```sh
cargo run --release -p un-desktop                 # the window: camera or a simulated table, demo lesson
cargo run --release -p un-cli -- cameras
cargo run --release -p un-cli -- run --camera 0                  # toys; one JSON line per state change
cargo run --release -p un-cli -- lesson --sim 3 --answers 3,2 --voice console   # a scripted lesson, no camera (words work too: Mia,yes,3)
cargo run --release -p un-cli -- lesson --camera 0 --language ru                # the real thing, headless
```

Desktop keys:

| Key | Action |
|---|---|
| drag | Mark the table area |
| `r` | Reset the table area |
| digits | Answer on the number pad |
| `space` | Repeat the prompt |
| `n` | Skip a task |
| `+` / `-` | Change the simulated table |

sherpa-onnx is linked as shared libraries: `libsherpa-onnx-c-api` and `libonnxruntime` sit next to
the executable. Cargo puts them in `target/<profile>/deps`, where the binaries also look. Ship
them beside the executable.

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

A product is a binary or a mobile library that links the engine and its strategy crates:
`un_desktop::run(registry)`, `un_cli::main()`, or `un-ffi` re-exported from a `staticlib` or
`cdylib`. `apps/un` and `apps/mobile` are exactly that.

## Platforms

| | Camera | Detector | Speech in | Speech out | UI |
|---|---|---|---|---|---|
| macOS | nokhwa (AVFoundation) | `un-vision` (ORT CPU/CoreML) | sherpa-onnx | sherpa-onnx (Piper) | egui |
| Windows | nokhwa (Media Foundation) | `un-vision` (ORT CPU/DirectML) | sherpa-onnx | sherpa-onnx (Piper) | egui |
| Linux | nokhwa (V4L2) | `un-vision` (ORT CPU) | sherpa-onnx | sherpa-onnx (Piper) | egui |
| iOS | AVFoundation | `un-vision` via FFI (ORT CoreML) | Apple Speech | AVSpeechSynthesizer | SwiftUI |
| Android | CameraX | `un-vision` via FFI (ORT) | SpeechRecognizer | TextToSpeech | Compose |

See `docs/architecture.md` for the reasoning behind each choice.

## Tests

```sh
cargo test --workspace            # state machines, parsers, decoders, the runtime end to end
cargo run -p un-cli -- check-speech
```

## Releases

Every release is started by hand from the Actions tab, or with `gh workflow run "<name>"`. The tag
is the version, as in onym-ios. A blank tag means the next patch of the latest `v*` tag
(`v0.1.9` → version 0.1.9, build 109). Each platform releases on its own:

| Workflow | What it ships |
|---|---|
| **Release iOS** | TestFlight, then the External group and Beta App Review once the build is processed; the tag and a GitHub Release. |
| **Release macOS TestFlight** | The Mac app (the iOS screens, `UnMac`) to TestFlight, then the External group and Beta App Review once processed. No notarisation. |
| **Release macOS** | The egui desktop app, `Un.app` (Apple silicon), signed with Developer ID and notarised, outside the App Store. |
| **Release Windows** | The Windows x64 alpha zip. |
| **Release Linux** | The Linux x86-64 alpha `.tar.gz` (glibc 2.39+). |
| **Release Android** | The app bundle for Google Play and an APK to install directly, both signed with the upload key. The bundle goes to a Play track (`-f track=internal\|alpha\|beta\|production\|none`). |
| **Play Store Metadata** | The Play listing's text and images from `fastlane/metadata/android`. |
| **TestFlight External** | Gives an uploaded build to the External group (`-f platform=osx` for the Mac app). |

macOS, macOS TestFlight, Windows, Linux and Android also take an existing tag (`-f tag=v0.1.9`).
They build that tag and attach the result (or a line in the notes) to its release.

The Mac app has the iOS app's bundle ID, so it is the macOS platform of the same App Store
Connect record (one purchase for both). Run it locally with `tools/ios.sh mac`. It uses the
system's camera and microphone (the camera chosen in macOS, an iPhone as a Continuity Camera
included) unless others are picked in its Settings (⌘,).

The apps ship from this repository with [unmath](https://github.com/sealsbroken/unmath) only. The
release notes record which unmath commit each build has. The toy detector comes from the
`models-v1` release.

Secrets:

| Secret | For |
|---|---|
| `ASC_KEY_ID`, `ASC_ISSUER_ID`, `ASC_KEY_CONTENT` | The App Store Connect API key (sealsbroken organisation, Admin role): TestFlight for iOS and the Mac, and macOS notarisation. |
| `MACOS_CERT_P12`, `MACOS_CERT_PASSWORD` | The Developer ID Application certificate. |
| `ANDROID_KEYSTORE_BASE64`, `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS`, `ANDROID_KEY_PASSWORD` | The Play upload key. |
| `PLAY_SERVICE_ACCOUNT_JSON` | Google Play uploads and metadata. Without it, the Play upload is skipped. |

## Licence

LGPL-3.0-or-later (`LICENSE`, with the GPL text it refers to in `LICENSE.GPL`).

Third-party components:

- ONNX Runtime, `ort`, egui, nokhwa, cpal: MIT/Apache-2.0.
- sherpa-onnx and the Zipformer models: Apache-2.0.
- Whisper and Silero VAD: MIT.
- The Piper voices. LJSpeech (English) is trained from scratch on public-domain recordings.
  Irina (Russian) is trained on RHVoice recordings whose model card says "Unknown" licence, and
  it is fine-tuned from Lessac, whose data is research-only. Every Russian Piper voice has one of
  these problems: Denis and Dmitri (CC0 data) are also fine-tuned from Lessac, and Ruslan's data
  is non-commercial. Many English ones do too (Lessac, Hi-Fi Captain). Before selling a desktop
  build, replace Irina with a Russian voice trained on data we may use.

The toy detector (`models/toys.onnx`) is not in this repository. Built with `tools/label.py` and
`tools/train_detector.py`, it is Apache-2.0 work:

- RF-DETR (Roboflow): the code and the nano weights are Apache-2.0.
- OWLv2 (Google, Apache-2.0) labels the photos, via Hugging Face Transformers (Apache-2.0).
- Open Images photos carry CC BY 2.0 (images) and CC BY 4.0 (annotations). Credit them wherever
  the model is distributed.

An app built on the detector can stay closed or go to the App Store.

The desktop voice is not LGPL-compatible for closed products. The prebuilt sherpa-onnx library
(`sherpa-onnx-c-api`, used by `un-speech` for the Piper voices) contains espeak-ng, which is
GPL-3.0, so a desktop app shipped with it is a combined work under the GPL, lessons included. A
closed desktop product needs the system voice instead of Piper, or the voice in a separate GPL
process. The mobile libraries leave `un-speech` out and use the platform's speech, so they are
not affected.
