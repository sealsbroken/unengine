#!/bin/zsh
# Builds the iOS app: the Rust library (apps/mobile: the engine with the unmath lessons, from the
# unmath checkout next to this repository), the Swift bindings, the Xcode project in apps/ios.
# For the simulator it also installs and launches the app.
#
#   rustup target add aarch64-apple-ios aarch64-apple-ios-sim && brew install xcodegen
#   tools/ios.sh sim [device-name]     # build, install and launch on a simulator (default: iPhone 17)
#   tools/ios.sh device                # build for an iPhone or iPad, then open the Xcode project to run it
#   tools/ios.sh run [device-name]     # build, install and launch on a connected iPhone/iPad (default: the first iPad)
#   tools/ios.sh test [device-name]    # the synthetic lesson tests (apps/ios/Tests) on a simulator
#   tools/ios.sh archive               # a signed Release archive: dist/ios/Un-<version>.xcarchive
#   tools/ios.sh prepare               # the device library, the bindings and the Xcode project, nothing built
#                                      #   (CI: fastlane archives, signs and uploads to TestFlight)
#
# The same app for the Mac (the UnMac target: the iOS screens in a window, sandboxed, Apple silicon):
#   rustup target add aarch64-apple-darwin
#   tools/ios.sh mac                   # build and launch it
#   tools/ios.sh mac-prepare           # the Mac library, the bindings and the Xcode project (CI)
#
# Versions come from the release tag (scripts/resolve-release-version.sh, as in onym-ios):
# RELEASE_VERSION=v0.1.7 -> version 0.1.7, build 107; without it, `git describe`.
#
# The simulator has no camera: the lesson starts and speaks, but sees an empty table.
set -euo pipefail

here=${0:a:h}/..
cd "$here"
mode=${1:-sim}
ios=$here/apps/ios
models=${UN_MODELS:-$here/models}
manifest=$here/apps/mobile/Cargo.toml
case $mode in
  sim | test) target=aarch64-apple-ios-sim; sdk=iphonesimulator ;;
  device | run | archive | prepare) target=aarch64-apple-ios; sdk=iphoneos ;;
  mac | mac-prepare) target=aarch64-apple-darwin; sdk=macosx ;;
  *) echo "usage: tools/ios.sh sim [device-name] | device | run [device-name] | test [device-name] | archive | prepare | mac | mac-prepare" >&2; exit 2 ;;
esac

# The deployment target of the Rust build (rustc's default for iOS is far older than the
# prebuilt ONNX Runtime, whose objects then need symbols the old libSystem lacks).
export IPHONEOS_DEPLOYMENT_TARGET=17.0 MACOSX_DEPLOYMENT_TARGET=14.0
[[ -d $here/../unmath ]] || { echo "the unmath lessons are missing: clone sealsbroken/unmath next to this repository (../unmath)" >&2; exit 1; }
[[ -f $models/toys.onnx ]] || { echo "no toy detector in $models: gh release download models-v1 -R sealsbroken/unengine -D $models" >&2; exit 1; }
cargo build --manifest-path "$manifest" --release --target $target
lib=$here/apps/mobile/target/$target/release/libun_mobile.a
rm -rf "$ios/Generated"
cargo run -q --manifest-path "$manifest" --bin uniffi-bindgen -- generate --library "$lib" --language swift --no-format --out-dir "$ios/Generated"
mkdir -p "$ios/Frameworks/$sdk" "$ios/Resources/models"
cp "$lib" "$ios/Frameworks/$sdk/"
# Only the toy detector; models from older builds (hand models, a language model) are removed.
rm -rf "$ios/Resources/models"/{hand_landmark.onnx,palm_detection.onnx,llm}
for f in toys.onnx toys.json; do
  cp "$models/$f" "$ios/Resources/models/"
done
rm -rf "$ios"/build/Build/Products/*/UnApp.app/models/{llm,hand_landmark.onnx,palm_detection.onnx}(N)
# iPhone and iPad: the Russian tutor's Piper voice and sherpa-onnx, which speaks it (its static
# library alone: it runs on the ONNX Runtime inside libun_mobile). The Mac app keeps Apple's voice.
if [[ $sdk != macosx ]]; then
  [[ -f $models/voice-ru/model.onnx ]] || { echo "no Russian voice in $models/voice-ru: gh release download models-v1 -R sealsbroken/unengine -p voice-ru.tar.gz -O - | tar -xz -C $models" >&2; exit 1; }
  rm -rf "$ios/Resources/models/voice-ru"
  cp -R "$models/voice-ru" "$ios/Resources/models/"
  sherpa_tag=v1.12.9
  sherpa_sha=d5891a2e57895ce22d0623c486809ea2e142ea35d61b38c17d2f51e1f0def98e
  # The archive sherpa-rs fetched for the simulator, else a download of the same one.
  sherpa=$HOME/Library/Caches/sherpa-rs/aarch64-apple-ios-sim/$sherpa_sha
  if [[ ! -d $sherpa/build-ios ]]; then
    sherpa=$here/vendor/sherpa-onnx-$sherpa_tag-ios
    if [[ ! -d $sherpa/build-ios ]]; then
      mkdir -p "$sherpa"
      curl -fsSL "https://github.com/k2-fsa/sherpa-onnx/releases/download/$sherpa_tag/sherpa-onnx-$sherpa_tag-ios.tar.bz2" -o "$sherpa.tar.bz2"
      echo "$sherpa_sha  $sherpa.tar.bz2" | shasum -a 256 -c - >/dev/null
      tar -xjf "$sherpa.tar.bz2" -C "$sherpa"
      rm "$sherpa.tar.bz2"
    fi
  fi
  slice=ios-arm64
  [[ $sdk == iphonesimulator ]] && slice=ios-arm64_x86_64-simulator
  cp "$sherpa/build-ios/sherpa-onnx.xcframework/$slice/libsherpa-onnx.a" "$ios/Frameworks/$sdk/"
  rm -rf "$ios/Frameworks/include"
  mkdir -p "$ios/Frameworks/include"
  cp -R "$sherpa/build-ios/sherpa-onnx.xcframework/Headers/sherpa-onnx" "$ios/Frameworks/include/"
fi

cd "$ios"
xcodegen generate
if [[ $mode == mac ]]; then
  xcodebuild -project UnApp.xcodeproj -scheme UnMac -configuration Debug -destination 'platform=macOS,arch=arm64' \
    -derivedDataPath build -allowProvisioningUpdates build -quiet
  open build/Build/Products/Debug/Un.app
  exit 0
fi
if [[ $mode == prepare || $mode == mac-prepare ]]; then
  echo "prepared: $ios/UnApp.xcodeproj"
  exit 0
fi
if [[ $mode == run ]]; then
  # A connected device by name (substring), else the first iPad; signed with the team in project.yml.
  wanted=${2:-iPad}
  json=$(mktemp)
  xcrun devicectl list devices --json-output "$json" >/dev/null
  udid=$(python3 -c "
import json, sys
devices = json.load(open('$json'))['result']['devices']
for d in devices:
    p, h = d.get('deviceProperties', {}), d.get('hardwareProperties', {})
    if h.get('reality') != 'physical':
        continue
    if '$wanted'.lower() in (p.get('name', '') + ' ' + h.get('marketingName', '') + ' ' + h.get('deviceType', '')).lower():
        print(h['udid']); break")
  rm -f "$json"
  [[ -n $udid ]] || { echo "no connected device matching '$wanted' (xcrun devicectl list devices)" >&2; exit 1; }
  xcodebuild -project UnApp.xcodeproj -scheme UnApp -configuration Debug -destination "id=$udid" \
    -derivedDataPath build -allowProvisioningUpdates build -quiet
  xcrun devicectl device install app --device "$udid" build/Build/Products/Debug-iphoneos/UnApp.app
  launch=()
  [[ -n ${UN_CONSOLE:-} ]] && launch+=(--console)
  xcrun devicectl device process launch --device "$udid" "${launch[@]}" app.onym.undigit
  exit 0
fi
if [[ $mode == archive ]]; then
  # The version is the release tag's; the build number MAJOR*10000 + MINOR*100 + PATCH.
  eval "$("$here/scripts/resolve-release-version.sh")"
  version=$MARKETING_VERSION
  build=$CURRENT_PROJECT_VERSION
  out=$here/dist/ios/Un-$version.xcarchive
  rm -rf "$out"
  xcodebuild -project UnApp.xcodeproj -scheme UnApp -configuration Release -destination 'generic/platform=iOS' \
    -archivePath "$out" -allowProvisioningUpdates MARKETING_VERSION="$version" CURRENT_PROJECT_VERSION="$build" archive -quiet
  echo "archive: $out (version $version, build $build)"
  echo "Upload: open it in Xcode (Window > Organizer > Distribute App), or export an .ipa with xcodebuild -exportArchive."
  exit 0
fi
if [[ $mode == device ]]; then
  echo "Open $ios/UnApp.xcodeproj, pick your team under Signing, choose the iPhone and press Run (or Archive)."
  open UnApp.xcodeproj
  exit 0
fi

name=${2:-iPhone 17}
udid=$(xcrun simctl list devices available -j | python3 -c "
import json, sys
devices = [d for v in json.load(sys.stdin)['devices'].values() for d in v if d['name'] == '$name']
print(devices[0]['udid'] if devices else '')")
[[ -n $udid ]] || { echo "no simulator named '$name' (xcrun simctl list devices)" >&2; exit 1; }
if [[ $mode == test ]]; then
  # The engine and the lessons through the Swift bindings, a scripted child, a simulated table.
  xcodebuild -project UnApp.xcodeproj -scheme UnApp -sdk iphonesimulator -destination "platform=iOS Simulator,id=$udid" \
    -derivedDataPath build CODE_SIGNING_ALLOWED=NO -only-testing:UnAppTests test -quiet
  # -quiet hides the passes; the result bundle has them.
  result=$(ls -td build/Logs/Test/*.xcresult | head -1)
  xcrun xcresulttool get test-results tests --path "$result" | python3 -c "
import json, sys
def walk(node):
    if node.get('nodeType') == 'Test Case':
        print(f\"  {node['result']:7} {node['name']}  ({node.get('duration', '')})\")
    for child in node.get('children', []):
        walk(child)
for node in json.load(sys.stdin)['testNodes']:
    walk(node)"
  exit 0
fi
xcodebuild -project UnApp.xcodeproj -scheme UnApp -sdk iphonesimulator -destination "platform=iOS Simulator,id=$udid" \
  -derivedDataPath build CODE_SIGNING_ALLOWED=NO build -quiet
xcrun simctl boot "$udid" 2>/dev/null || true
# Recent Xcodes show simulators from Xcode itself (Window > Devices and Simulators); older ones
# ship Simulator.app. Either way the device is booted and the app runs.
open -a Simulator 2>/dev/null || true
xcrun simctl install "$udid" build/Build/Products/Debug-iphonesimulator/UnApp.app
xcrun simctl launch "$udid" app.onym.undigit
echo "screenshot: xcrun simctl io $udid screenshot shot.png"
