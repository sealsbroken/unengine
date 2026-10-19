#!/bin/zsh
# An App Store app preview from a screen recording of the app: the chosen segments joined and
# encoded as App Store Connect wants them (15-30 s, 30 fps, H.264, stereo AAC), at the device's
# preview size, into fastlane/previews/<ios|mac>/en-US/, where the App Store Metadata workflow
# uploads them from (-f previews=true). The primary language's previews serve every locale.
#
#   tools/app-preview.sh iphone ~/Desktop/RPReplay.MP4 1-3 5-13.5 14-22 26.5-36.5
#   tools/app-preview.sh ipad   recording.mov 0-28
#   tools/app-preview.sh mac    recording.mov 2-30
#
# Sizes (portrait, landscape for the Mac): iPhone 886x1920 (the 6.9" set, shown on every iPhone),
# iPad 1200x1600 (13"), Mac 1920x1080. A recording of another aspect is scaled to cover the size
# and cropped in the middle. Record with Control Center's screen recording (iPhone, iPad) or
# QuickTime (the Mac app's window); App Review takes only footage of the app itself.
#
# POSTER=20: the still App Store shows before the preview plays, as seconds into the preview
# (default 10), kept next to it as <preview>.poster for the upload.
set -euo pipefail

here=${0:a:h}/..
device=${1:?usage: tools/app-preview.sh iphone|ipad|mac <recording> <start-end>...}
source=${2:?the screen recording}
shift 2
(( $# > 0 )) || { echo "give at least one segment, start-end in seconds" >&2; exit 2; }
case $device in
  iphone) size=886x1920; dir=ios; type=IPHONE_67 ;;
  ipad) size=1200x1600; dir=ios; type=IPAD_PRO_3GEN_129 ;;
  mac) size=1920x1080; dir=mac; type=DESKTOP ;;
  *) echo "device: iphone, ipad or mac" >&2; exit 2 ;;
esac
w=${size%x*}; h=${size#*x}

# The segments, trimmed and joined (video and audio together; a recording without sound gets a
# silent track: App Store Connect expects one).
has_audio=$(ffprobe -v error -select_streams a -show_entries stream=index -of csv=p=0 "$source" | head -1)
filter="" labels="" total=0 n=0
for segment in "$@"; do
  from=${segment%-*}; to=${segment#*-}
  filter+="[0:v]trim=$from:$to,setpts=PTS-STARTPTS[v$n];"
  if [[ -n $has_audio ]]; then
    filter+="[0:a]atrim=$from:$to,asetpts=PTS-STARTPTS[a$n];"
  else
    filter+="anullsrc=r=48000:cl=stereo,atrim=0:$((to - from))[a$n];"
  fi
  labels+="[v$n][a$n]"
  total=$((total + to - from))
  n=$((n + 1))
done
(( total >= 15 && total <= 30 )) || { echo "the preview is ${total} s; App Store Connect takes 15 to 30" >&2; exit 1; }
filter+="${labels}concat=n=$n:v=1:a=1[v][a];"
filter+="[v]fps=30,scale=$w:$h:force_original_aspect_ratio=increase,crop=$w:$h,setsar=1,scale=out_range=tv,format=yuv420p[out]"

out=$here/fastlane/previews/$dir/en-US/$device-$type.mp4
mkdir -p "${out:h}"
ffmpeg -v error -y -i "$source" -filter_complex "$filter" -map "[out]" -map "[a]" \
  -c:v libx264 -profile:v high -level 4.0 -color_range tv -colorspace bt709 -color_primaries bt709 -color_trc bt709 -preset slow -b:v 10M -maxrate 12M -bufsize 20M \
  -c:a aac -b:a 256k -ar 48000 -ac 2 -movflags +faststart "$out"
poster=${POSTER:-10}
printf '00:00:%02d:00\n' $poster > "${out:r}.poster"
echo "$out: $(ffprobe -v error -show_entries format=duration -of csv=p=0 "$out") s, $size"
