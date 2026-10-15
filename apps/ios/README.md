# Un for iOS

A SwiftUI shell over the shared Rust engine. The phone's camera frames go to the shared
perception (`UnPerception`, ONNX Runtime on the CPU/CoreML) and the lesson runs in the shared
engine (`UnSession`); speech uses Apple's on-device synthesiser and recogniser through the
`SpeechOutputCallback` / `SpeechInputCallback` interfaces. Swap in Vision's hand pose or
sherpa-onnx's Swift package by implementing the same interfaces.

## Build

The product build (engine plus the Unmethod lessons) is one command in the unmethod checkout:
`tools/ios.sh sim` builds, installs and launches on a simulator, `tools/ios.sh device` builds
for an iPhone and opens the Xcode project. By hand, for the engine alone with the demo strategy:

```sh
rustup target add aarch64-apple-ios aarch64-apple-ios-sim && brew install xcodegen
cargo build -p un-ffi --release --target aarch64-apple-ios-sim        # aarch64-apple-ios for a phone
cargo run -p un-ffi --bin uniffi-bindgen -- generate \
    --library ../../target/aarch64-apple-ios-sim/release/libun_ffi.a --language swift --no-format --out-dir Generated
mkdir -p Frameworks/iphonesimulator Resources/models && cp ../../target/aarch64-apple-ios-sim/release/libun_ffi.a Frameworks/iphonesimulator/
cp ../../models/{caps,detector}.{onnx,json} ../../models/{hand_landmark,palm_detection}.onnx Resources/models/
xcodegen generate && open UnApp.xcodeproj
```

With un-ffi alone, link `-lun_ffi` in `project.yml` and drop the `unmethod_mobile_init()` call
in `UnApp.swift` (it is the product library's registration of its lessons).

Notes:

* `--no-format`: swift-format spins for a very long time on the generated file.
* No `-all_load`: the prebuilt ONNX Runtime carries some objects twice and the link would
  fail; strategies are therefore registered explicitly through `unmethod_mobile_init()`.
* `Resources/models` is bundled as a folder reference, so the app finds `models/caps.onnx`.
* The simulator has no camera and no microphone: the tutor speaks, sees an empty table and hears
  nothing, which exercises the no-reaction dialogue. For the real lesson run on a phone:
  `tools/ios.sh device`, then in Xcode pick your team under Signing & Capabilities and press Run.
* The lesson starts by itself once the permission prompts are answered;
  `xcrun simctl io <udid> screenshot shot.png` shows it without a Simulator window.

## Tests

`Tests/` holds synthetic lesson tests: lesson 1 driven end to end through the Swift bindings
with a scripted child (`ScriptedSpeech`, lines queued by the test), a simulated table
(`UnSession.observe`) and the tutor's lines written down (`RecordedVoice`). No camera, no
microphone, no host app; the engine's own timers run, so a count takes the beat and a half the
tutor waits after the last number. `tools/ios.sh test` in the unmethod checkout runs them on the
simulator (`xcodebuild test -scheme UnApp -only-testing:UnAppTests`).

Generated files: `Generated/un_ffi.swift`, `Generated/un_ffiFFI.h`, `Generated/un_ffiFFI.modulemap`.
ONNX Runtime for iOS is linked statically by the `ort` crate (download-binaries). sherpa-onnx is
not needed on iOS because speech goes through Apple's APIs (`un-app` is built without its `speech`
feature); if you prefer Piper voices, add the `sherpa-onnx` Swift package and implement
`SpeechOutputCallback` over it.
