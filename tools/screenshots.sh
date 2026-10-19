#!/bin/zsh
# The App Store screenshots, from the app's screenshot mode (apps/ios/UnApp/Screenshot.swift):
# each screen (the children, a child's trail, a lesson) in English and Russian, saved by the app
# itself, into fastlane/screenshots/<ios|mac>/<locale>/, where the App Store Metadata workflow
# uploads them from (-f screenshots=true).
#
#   tools/screenshots.sh photo [ipad|mac]   # the table photo: a still from the iPad's (default)
#                                           #   or the Mac's camera, looking at real toys →
#                                           #   fastlane/screenshots/source/table.jpg
#   tools/screenshots.sh ios                # iPhone 6.9" and iPad 13" simulators, with that photo
#   tools/screenshots.sh mac                # the Mac app, 1280x800 points, with the Mac's camera
#                                           #   (UN_SCREENSHOT_PHOTO=1: with the photo instead)
#   tools/screenshots.sh all                # ios and mac
#
# The simulators have no camera: the photo stands in, and its frames still go through the real
# detector, so the boxes and the numbers are what the app would draw. Point the camera at a few
# toys on a table, all of them well inside the picture.
set -euo pipefail

here=${0:a:h}/..
cd "$here"
mode=${1:-all}
ios=$here/apps/ios
out=$here/fastlane/screenshots
share=/tmp/un-screenshots          # the folder the app reads the photo from and saves into
photo=$out/source/table.jpg
screens=(children trail lesson)
languages=(en ru)
delay=${UN_SCREENSHOT_DELAY:-14}   # seconds until the app saves the screen (the lesson needs the detector)
mkdir -p "$share"

locale_dir() { [[ $1 == en ]] && echo en-US || echo ru; }

# The app's PNG as a JPEG (quality 92, what App Store Connect takes): a camera picture is about
# a tenth of the size, and the screenshots live in the repository for the upload workflow.
save_shot() {
  mkdir -p "${2:h}"
  sips -s format jpeg -s formatOptions 92 "$share/screenshot.png" --out "$2" >/dev/null
}

# The app saves screenshot.png in $share and quits; wait for it (up to $1 seconds).
await_shot() {
  local i
  for i in $(seq ${1:-60}); do
    [[ -f $share/screenshot.png ]] && { sleep 1; return 0; }
    sleep 1
  done
  echo "no screenshot after ${1:-60} s" >&2
  return 1
}

build_sim() {
  zsh tools/ios.sh sim "$1" >/dev/null
}

sim_udid() {
  local name=$1 type=$2 udid
  udid=$(xcrun simctl list devices available -j | python3 -c "
import json, sys
devices = [d for v in json.load(sys.stdin)['devices'].values() for d in v if d['name'] == '$name']
print(devices[0]['udid'] if devices else '')")
  if [[ -z $udid ]]; then
    udid=$(xcrun simctl create "$name" "com.apple.CoreSimulator.SimDeviceType.$type")
  fi
  echo "$udid"
}

shoot_ios() {
  [[ -f $photo ]] || { echo "no table photo: run tools/screenshots.sh photo first ($photo)" >&2; exit 1; }
  cp "$photo" "$share/screenshot-table.jpg"
  # The sizes the App Store asks for: iPhone 6.9" (1320x2868) and iPad 13" (2064x2752).
  local spec name type dir udid lang screen n
  for spec in "Un Screenshots iPhone:iPhone-17-Pro-Max:iphone" "Un Screenshots iPad:iPad-Pro-13-inch-M5-12GB:ipad"; do
    name=${spec%%:*}; type=${${spec#*:}%%:*}; dir=${spec##*:}
    udid=$(sim_udid "$name" "$type")
    xcrun simctl boot "$udid" 2>/dev/null || true
    xcrun simctl bootstatus "$udid" -b >/dev/null
    for p in camera microphone speech-recognition; do xcrun simctl privacy "$udid" grant $p app.onym.undigit 2>/dev/null || true; done
    build_sim "$name"
    for lang in $languages; do
      n=1
      for screen in $screens; do
        rm -f "$share/screenshot.png"
        SIMCTL_CHILD_UN_SCREENSHOT=$screen SIMCTL_CHILD_UN_LANGUAGE=$lang SIMCTL_CHILD_UN_SCREENSHOT_DELAY=$delay \
          xcrun simctl launch --terminate-running-process "$udid" app.onym.undigit >/dev/null
        await_shot 90
        save_shot "$share/screenshot.png" "$out/ios/$(locale_dir $lang)/${dir}-0${n}-${screen}.jpg"
        echo "ios $dir $lang $screen"
        n=$((n + 1))
      done
    done
    xcrun simctl shutdown "$udid" 2>/dev/null || true
  done
  rm -f "$share/screenshot-table.jpg"
}

# Runs the Mac app through launchd, as Finder would: launched from a terminal, macOS charges the
# app's permission requests to the terminal and kills it. Saved window state is ignored (a run
# that was killed can leave none).
run_mac() {
  local env_xml="" k v plist=$share/launch.plist label=app.onym.undigit.screenshot
  for k v in "$@"; do env_xml+="<key>$k</key><string>$v</string>"; done
  cat > "$plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>$label</string>
<key>ProgramArguments</key><array><string>$ios/build/Build/Products/Debug/Un.app/Contents/MacOS/Un</string><string>-ApplePersistenceIgnoreState</string><string>YES</string></array>
<key>EnvironmentVariables</key><dict>$env_xml</dict>
<key>RunAtLoad</key><true/>
</dict></plist>
PLIST
  launchctl bootout gui/$(id -u)/$label 2>/dev/null || true
  launchctl bootstrap gui/$(id -u) "$plist"
  await_shot 90 || true
  launchctl bootout gui/$(id -u)/$label 2>/dev/null || true
}

build_mac() {
  pkill -x Un 2>/dev/null || true
  zsh tools/ios.sh mac-prepare >/dev/null
  xcodebuild -project "$ios/UnApp.xcodeproj" -scheme UnMac -configuration Debug -destination 'platform=macOS,arch=arm64' \
    -derivedDataPath "$ios/build" -allowProvisioningUpdates build -quiet
}

shoot_mac() {
  build_mac
  rm -f "$share/screenshot-table.jpg"
  [[ -n ${UN_SCREENSHOT_PHOTO:-} ]] && cp "$photo" "$share/screenshot-table.jpg"
  local lang screen n
  for lang in $languages; do
    n=1
    for screen in $screens; do
      rm -f "$share/screenshot.png"
      # 1280x800 points: 2560x1600 pixels, an App Store Mac size that fits a MacBook Air's screen.
      run_mac UN_SCREENSHOT $screen UN_LANGUAGE $lang UN_SCREENSHOT_SIZE 1280x800 UN_SCREENSHOT_DELAY $delay
      [[ -f $share/screenshot.png ]] || { echo "mac $lang $screen: no screenshot" >&2; exit 1; }
      save_shot "$share/screenshot.png" "$out/mac/$(locale_dir $lang)/mac-0${n}-${screen}.jpg"
      echo "mac $lang $screen"
      n=$((n + 1))
    done
  done
  rm -f "$share/screenshot-table.jpg"
}

# A still from a real camera looking at toys, for the simulators.
take_photo() {
  local from=${1:-ipad} udid json
  rm -f "$share/screenshot.png" "$share/camera.jpg" "$share/screenshot-table.jpg"
  mkdir -p "${photo:h}"
  if [[ $from == mac ]]; then
    build_mac
    run_mac UN_SCREENSHOT lesson UN_LANGUAGE en UN_SCREENSHOT_DELAY $delay
    [[ -f $share/camera.jpg ]] || { echo "the Mac's camera gave no still" >&2; exit 1; }
    cp "$share/camera.jpg" "$photo"
  else
    # The connected iPad (tools/ios.sh run builds, installs and starts it), then screenshot mode
    # through devicectl; the app keeps its files in Documents/screenshots.
    zsh tools/ios.sh run iPad >/dev/null
    json=$(mktemp)
    xcrun devicectl list devices --json-output "$json" >/dev/null
    udid=$(python3 -c "
import json
for d in json.load(open('$json'))['result']['devices']:
    p, h = d.get('deviceProperties', {}), d.get('hardwareProperties', {})
    if h.get('reality') == 'physical' and 'ipad' in (p.get('name', '') + h.get('deviceType', '')).lower():
        print(h['udid']); break")
    rm -f "$json"
    [[ -n $udid ]] || { echo "no iPad connected" >&2; exit 1; }
    xcrun devicectl device process launch --device "$udid" --terminate-existing \
      --environment-variables "{\"UN_SCREENSHOT\": \"lesson\", \"UN_LANGUAGE\": \"en\", \"UN_SCREENSHOT_DELAY\": \"$delay\"}" \
      app.onym.undigit >/dev/null
    sleep $((delay + 8))
    xcrun devicectl device copy from --device "$udid" --domain-type appDataContainer --domain-identifier app.onym.undigit \
      --source Documents/screenshots/camera.jpg --destination "$photo" >/dev/null
  fi
  echo "photo: $photo ($(sips -g pixelWidth -g pixelHeight "$photo" | awk '/pixel/ {printf "%s ", $2}'))"
}

case $mode in
  photo) take_photo "${2:-ipad}" ;;
  ios) shoot_ios ;;
  mac) shoot_mac ;;
  all) shoot_ios; shoot_mac ;;
  *) echo "usage: tools/screenshots.sh photo [ipad|mac] | ios | mac | all" >&2; exit 2 ;;
esac
