# Un for Android

A Jetpack Compose app over the shared Rust library (apps/mobile: the engine with the unmath
lessons), with the iOS app's screens one for one: the children (cards with progress, add a
child, remove), a child's adventures as a trail, and the lesson: the camera picture with the toy
boxes and number badges, the tutor panel, the back button and toolbar (pause, repeat, skip,
language), the number pad when the microphone can't hear, the count ticket and the end screens
("Продолжим?"). Perception and the lesson run in Rust (`UnPerception`, `UnSession`, ONNX Runtime
on the CPU); speech uses Android's TextToSpeech and SpeechRecognizer.

## Build

```sh
cargo install cargo-ndk && rustup target add aarch64-linux-android
sdkmanager 'ndk;29.0.14206865'           # or any NDK; tools/android.sh takes the newest
gh release download models-v1 -R sealsbroken/unengine -p 'toys.*' -D models
tools/android.sh install                 # the library, bindings, models; a debug build on the device or emulator
tools/android.sh bundle                  # the unsigned release bundle (what CI signs and uploads)
```

The unmath checkout must sit next to this repository (`../unmath`). The script builds
`libun_mobile.so` for arm64 (16 KB pages), copies the NDK's `libc++_shared.so` next to it,
generates the Kotlin bindings into `app/src/main/java/uniffi/` and copies the toy detector into
`app/src/main/assets/models/` (all git-ignored). An arm64 emulator works (Apple silicon), with
the emulator's virtual scene as the camera.

Gradle needs a JDK 17 or 21 (`JAVA_HOME`; Android Studio's bundled Java 25 is too new for the
Kotlin plugin). The release build is shrunk by R8; `proguard-rules.pro` keeps JNA and the
UniFFI bindings, which are found by reflection.

## Release

`Release Android` (.github/workflows/release-android.yml) builds the bundle and an APK at the tag,
signs both with the upload key, attaches them to the tag's GitHub Release and uploads the bundle
to a Google Play track. The APK is for installing without Play; a Play install is signed with
Play's key, so one does not update the other. The first bundle is uploaded by hand in the Play Console (it enrols Play App Signing);
`play/icon-512.png` is the store icon.
