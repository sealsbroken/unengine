#!/bin/zsh
# Builds the Android app: the Rust library (apps/mobile: the engine with the unmath lessons, from
# the unmath checkout next to this repository) for arm64, the Kotlin bindings, the toy detector
# in the assets, then Gradle in apps/android.
#
#   cargo install cargo-ndk && rustup target add aarch64-linux-android
#   tools/android.sh prepare     # the library, libc++_shared, the bindings and the models, nothing built
#                                #   (CI: Gradle bundles, jarsigner signs, Play uploads)
#   tools/android.sh install     # prepare, then a debug build installed and launched on the device
#                                #   or emulator adb sees (an arm64 one: the library is arm64 only)
#   tools/android.sh bundle      # prepare, then the unsigned release bundle (app-release.aab)
#
# Needs the NDK (ANDROID_NDK_HOME, else the newest under $ANDROID_HOME/ndk) and the SDK
# (ANDROID_HOME, default ~/Library/Android/sdk). Versions come from the release tag
# (RELEASE_VERSION=v0.1.9 -> versionName 0.1.9, versionCode 109; without it, `git describe`).
set -euo pipefail

here=${0:a:h}/..
cd "$here"
mode=${1:-prepare}
android=$here/apps/android
app=$android/app/src/main
models=${UN_MODELS:-$here/models}
manifest=$here/apps/mobile/Cargo.toml
case $mode in
  prepare | install | bundle) ;;
  *) echo "usage: tools/android.sh prepare | install | bundle" >&2; exit 2 ;;
esac

export ANDROID_HOME=${ANDROID_HOME:-$HOME/Library/Android/sdk}
if [[ -z ${ANDROID_NDK_HOME:-} ]]; then
  ndks=($ANDROID_HOME/ndk/*(N/On))
  [[ ${#ndks} -gt 0 ]] || { echo "no NDK under $ANDROID_HOME/ndk: sdkmanager 'ndk;29.0.14206865'" >&2; exit 1; }
  export ANDROID_NDK_HOME=${ndks[1]}
fi
[[ -d $here/../unmath ]] || { echo "the unmath lessons are missing: clone sealsbroken/unmath next to this repository (../unmath)" >&2; exit 1; }
[[ -f $models/toys.onnx ]] || { echo "no toy detector in $models: gh release download models-v1 -R sealsbroken/unengine -D $models" >&2; exit 1; }

# The library; its C++ runtime (ONNX Runtime is C++) goes next to it.
rm -rf "$app/jniLibs"
cargo ndk -t arm64-v8a --platform 26 -o "$app/jniLibs" build --manifest-path "$manifest" --release
rm -f "$app/jniLibs/arm64-v8a/libun_ffi.so"   # un-ffi's own cdylib: everything is in libun_mobile.so
cp "$(print -l $ANDROID_NDK_HOME/toolchains/llvm/prebuilt/*/sysroot/usr/lib/aarch64-linux-android/libc++_shared.so | head -1)" \
  "$app/jniLibs/arm64-v8a/"
lib=$app/jniLibs/arm64-v8a/libun_mobile.so
rm -rf "$app/java/uniffi"
cargo run -q --manifest-path "$manifest" --bin uniffi-bindgen -- generate --library "$lib" --language kotlin --no-format \
  --out-dir "$app/java"
# Only the toy detector.
rm -rf "$app/assets/models"
mkdir -p "$app/assets/models"
for f in toys.onnx toys.json; do
  cp "$models/$f" "$app/assets/models/"
done

cd "$android"
gradle=(./gradlew --no-daemon -q)
case $mode in
  prepare) echo "prepared: $android" ;;
  install)
    "${gradle[@]}" :app:installDebug
    "$ANDROID_HOME/platform-tools/adb" shell am start -n app.onym.un/.MainActivity
    ;;
  bundle)
    "${gradle[@]}" :app:bundleRelease
    echo "bundle: $android/app/build/outputs/bundle/release/app-release.aab (unsigned)"
    ;;
esac
