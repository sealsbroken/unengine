# Un for iOS

A SwiftUI shell over the shared Rust engine, for iPhone and iPad (portrait, full screen). The phone's camera frames go to the shared
perception (`UnPerception`, ONNX Runtime on the CPU/CoreML) and the lesson runs in the shared
engine (`UnSession`); speech goes through the `SpeechOutputCallback` / `SpeechInputCallback` interfaces: Apple's
on-device recogniser, and Apple's synthesiser except for Russian on iPhone and iPad, which the
tutor says in a Piper voice fine-tuned on recordings of an adult man's voice (`models/voice-ru`;
`UnApp/PiperVoice.swift`, through sherpa-onnx's C library, which tools/ios.sh fetches).

## Build

The app (the engine plus the unmath lessons, `apps/mobile`) is one command in this repository,
with [unmath](https://github.com/sealsbroken/unmath) cloned next to it (`../unmath`) and the toy
detector and the Russian voice in `models/` (`gh release download models-v1 -R sealsbroken/unengine -p 'toys.*' -D models`,
then `gh release download models-v1 -R sealsbroken/unengine -p voice-ru.tar.gz -O - | tar -xz -C models`):

```sh
tools/ios.sh sim            # build, install and launch on a simulator ("iPhone 17"; tools/ios.sh sim "iPhone 17 Pro")
tools/ios.sh run [name]     # build, install and launch on a connected iPhone/iPad (default: the first iPad)
UN_CONSOLE=1 tools/ios.sh run   # the same, with the app's console
tools/ios.sh test           # the lesson tests (Tests/) on a simulator: a scripted child, a simulated table
tools/ios.sh device         # build, then open the Xcode project
tools/ios.sh mac            # the same app as a Mac app (the UnMac target): build and launch it
```

The Mac app is these screens in a window: `UnApp/Platform.swift` holds the few differences (the
camera preview view, keeping the screen awake, the lesson over the whole window). Its Settings
window (Un ▸ Settings…, ⌘,; `UnApp/MacSettings.swift`) picks the camera and the microphone; both
follow the system's choice by default (the camera chosen in macOS, an iPhone as a Continuity
Camera included; the default input). It is sandboxed (`UnMac.entitlements`), Apple silicon only,
and goes to TestFlight with **Release macOS TestFlight**.

TestFlight: the **Release iOS** workflow (Actions tab → Run workflow, or `gh workflow run "Release iOS"`)
builds on a GitHub macOS runner with unmath's main next to this repository, signs automatically with an App Store Connect API key (no certificate repository),
uploads to TestFlight, then tags the commit and creates a GitHub Release (no IPA attached: the
repository is public). As in onym-ios the tag is the version: leave it blank for the next patch of the
latest `v*` tag; `v0.1.7` gives version 0.1.7, build 107 (`scripts/resolve-release-version.sh`). Secrets
and the first-run setup are listed at the top of `.github/workflows/release-ios.yml`.

By hand, for the engine alone with the demo strategy:

```sh
rustup target add aarch64-apple-ios aarch64-apple-ios-sim && brew install xcodegen
cargo build -p un-ffi --release --target aarch64-apple-ios-sim        # aarch64-apple-ios for a phone
cargo run -p un-ffi --bin uniffi-bindgen -- generate \
    --library ../../target/aarch64-apple-ios-sim/release/libun_ffi.a --language swift --no-format --out-dir Generated
mkdir -p Frameworks/iphonesimulator Resources/models && cp ../../target/aarch64-apple-ios-sim/release/libun_ffi.a Frameworks/iphonesimulator/
cp ../../models/toys.{onnx,json} Resources/models/
xcodegen generate && open UnApp.xcodeproj
```

With un-ffi alone, link `-lun_ffi` in `project.yml` and drop the `un_mobile_init()` call
in `UnApp.swift` (it is un-mobile's registration of the unmath lessons).

Notes:

* `--no-format`: swift-format spins for a very long time on the generated file.
* No `-all_load`: the prebuilt ONNX Runtime carries some objects twice and the link would
  fail; strategies are therefore registered explicitly through `un_mobile_init()`.
* `Resources/models` is bundled as a folder reference, so the app finds `models/toys.onnx`.
* The simulator has no camera and no microphone: the tutor speaks, sees an empty table and hears
  nothing, which exercises the no-reaction dialogue. For the real lesson run on a phone:
  `tools/ios.sh device`, then in Xcode pick your team under Signing & Capabilities and press Run.
* A debug build keeps the last five camera frames and the detections of the latest one in the
  app's Documents/frames, and prints the time per frame to the console
  (`UN_CONSOLE=1 tools/ios.sh run`); pull them with
  `xcrun devicectl device copy from --device <udid> --domain-type appDataContainer --domain-identifier app.onym.undigit --source Documents/frames --destination frames`.
* A debug build skips the introductions (the child's and the tutor's names) and goes straight
  to the lesson, greeting a stand-in child by name; a release build asks.
* The lesson starts by itself once the permission prompts are answered;
  `xcrun simctl io <udid> screenshot shot.png` shows it without a Simulator window.

## Tests

`Tests/` holds synthetic lesson tests: lesson 1 with toys driven end to end through the Swift
bindings with a scripted child (`ScriptedSpeech`, lines queued by the test), a simulated table
(`UnSession.observe`) and the tutor's lines written down (`RecordedVoice`). No camera, no
microphone, no host app; the engine's own timers run, so a count takes the beat and a half the
tutor waits after the last number. `tools/ios.sh test` runs them on the
simulator (`xcodebuild test -scheme UnApp -only-testing:UnAppTests`).

Generated files: `Generated/un_ffi.swift`, `Generated/un_ffiFFI.h`, `Generated/un_ffiFFI.modulemap`.
ONNX Runtime for iOS is linked statically by the `ort` crate (download-binaries). `un-app` is built
without its `speech` feature: the Russian Piper voice is sherpa-onnx's own static library
(`Frameworks/<sdk>/libsherpa-onnx.a`, its C API header in `Frameworks/include`), called from Swift,
whose ONNX Runtime calls resolve to the one in libun_mobile.
