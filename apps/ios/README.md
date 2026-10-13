# Un for iOS

A SwiftUI shell over the shared Rust engine. The phone's camera frames go to the shared
perception (`UnPerception`, ONNX Runtime on the CPU/CoreML) and the lesson runs in the shared
engine (`UnSession`); speech uses Apple's on-device synthesiser and recogniser through the
`SpeechOutputCallback` / `SpeechInputCallback` interfaces. Swap in Vision's hand pose or
sherpa-onnx's Swift package by implementing the same interfaces.

## Build

```sh
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
cargo build -p un-ffi --release --target aarch64-apple-ios          # or the unmethod mobile crate for the product
cargo run -p un-ffi --bin uniffi-bindgen -- generate \
    --library ../../target/aarch64-apple-ios/release/libun_ffi.a --language swift --out-dir Generated
mkdir -p Frameworks && cp ../../target/aarch64-apple-ios/release/libun_ffi.a Frameworks/
cp -R ../../models Resources/models                                  # detector, hand models
brew install xcodegen && xcodegen generate && open UnApp.xcodeproj
```

Generated files: `Generated/un_ffi.swift`, `Generated/un_ffiFFI.h`, `Generated/un_ffiFFI.modulemap`.
For a universal simulator library, `lipo` the `aarch64-apple-ios-sim` and `x86_64-apple-ios` builds,
or package both as an `.xcframework` with `xcodebuild -create-xcframework`.

ONNX Runtime for iOS is linked statically by the `ort` crate (download-binaries). sherpa-onnx is
not needed on iOS because speech goes through Apple's APIs; if you prefer Piper voices, add the
`sherpa-onnx` Swift package and implement `SpeechOutputCallback` over it.
