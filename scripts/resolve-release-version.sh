#!/usr/bin/env bash
#
# Resolve the app's MARKETING_VERSION + CURRENT_PROJECT_VERSION from a
# single source of truth — the GitHub release tag — and emit them on
# stdout as `KEY=value` lines.
#
# Resolution order (the same scheme as onym-ios and onym-android):
#
#   1. env RELEASE_VERSION      — `release.yml` pipes the dispatch
#                                 input `tag` straight here at archive
#                                 time. Canonical CI path.
#   2. `git describe --tags --match 'v*' --abbrev=7`
#                               — local dev between tags renders as
#                                 `0.0.10-3-gca6471b`, handy in bug
#                                 reports.
#   3. Fallback `v0.0.0-dev`    — shallow clones, no-git sandboxes,
#                                 fresh repos before the first tag.
#
# CURRENT_PROJECT_VERSION = MAJOR*10000 + MINOR*100 + PATCH (clamped to
# ≥1 so AGP-style "0 is invalid" guards on the iOS side don't bite).
# Monotonic across `v0.x.y` and across the future jump to `v0.1.0`.
#
# Usage:
#
#   eval "$(scripts/resolve-release-version.sh)"
#   export MARKETING_VERSION CURRENT_PROJECT_VERSION
#
# Or in GitHub Actions:
#
#   scripts/resolve-release-version.sh >> "$GITHUB_OUTPUT"

set -euo pipefail

raw="${RELEASE_VERSION:-}"
if [ -z "$raw" ]; then
  raw="$(git describe --tags --match 'v*' --abbrev=7 2>/dev/null || true)"
fi
if [ -z "$raw" ]; then
  raw="v0.0.0-dev"
fi

# Strip leading `v` (Play / About-screen convention) and any
# `-N-gXXXX` dev suffix before parsing the numeric components.
name="${raw#v}"
base="${name%%-*}"

major=0
minor=0
patch=0
IFS='.' read -r m1 m2 m3 <<<"$base"
[[ "$m1" =~ ^[0-9]+$ ]] && major="$m1"
[[ "$m2" =~ ^[0-9]+$ ]] && minor="$m2"
[[ "$m3" =~ ^[0-9]+$ ]] && patch="$m3"

code=$((major * 10000 + minor * 100 + patch))
[ "$code" -lt 1 ] && code=1

printf 'MARKETING_VERSION=%s\n' "$name"
printf 'CURRENT_PROJECT_VERSION=%s\n' "$code"
