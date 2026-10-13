# Un for Android

A Jetpack Compose shell over the shared Rust engine. CameraX frames go to the shared
perception (`UnPerception`, ONNX Runtime; NNAPI/XNNPACK through `un-vision` features) and the
lesson runs in the shared engine (`UnSession`). Speech uses Android's TextToSpeech and
SpeechRecognizer through the `SpeechOutputCallback` / `SpeechInputCallback` interfaces; the
sherpa-onnx Kotlin package is a drop-in alternative for Piper voices and on-device Zipformer.

## Build

```sh
cargo install cargo-ndk
rustup target add aarch64-linux-android
cargo ndk -t arm64-v8a -o app/src/main/jniLibs build -p un-ffi --release      # or the unmethod mobile crate
cargo run -p un-ffi --bin uniffi-bindgen -- generate \
    --library ../../target/aarch64-linux-android/release/libun_ffi.so --language kotlin --out-dir app/src/main/java
mkdir -p app/src/main/assets/models && cp ../../models/*.onnx ../../models/*.json app/src/main/assets/models/
./gradlew installDebug
```

The generated Kotlin lands in `app/src/main/java/uniffi/un_ffi/`. ONNX Runtime for Android is
linked by the `ort` crate (download-binaries); sherpa-onnx is not needed on Android because
speech goes through the platform APIs.
