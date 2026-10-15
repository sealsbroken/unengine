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
    --library ../../target/aarch64-linux-android/release/libun_ffi.so --language kotlin --no-format --out-dir app/src/main/java
mkdir -p app/src/main/assets/models && cp ../../models/*.onnx ../../models/*.json app/src/main/assets/models/
./gradlew installDebug
```

To check the Kotlin without a phone, the bindings can be generated from a host build of
`un-ffi` (`cargo build -p un-ffi --release`, then `--library ../../target/release/libun_ffi.dylib`)
and compiled with `gradle :app:compileDebugKotlin`. Kotlin 2.0 does not run on Java 25, which
is what Android Studio bundles, so point `JAVA_HOME` at a JDK 21 (Android Studio's older
`jbr-21` under `~/Library/Java/JavaVirtualMachines` does).

The screen mirrors the iOS one: the tutor's line and status, the camera picture (CameraX
`PreviewView`, frames rotated upright so the overlay maps one to one) with the table overlay,
the number pad, start/end, repeat, skip and the EN/RU switch; the lesson starts by itself once
the camera permission is answered.

The generated Kotlin lands in `app/src/main/java/uniffi/un_ffi/`. ONNX Runtime for Android is
linked by the `ort` crate (download-binaries); sherpa-onnx is not needed on Android because
speech goes through the platform APIs.
