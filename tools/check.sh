#!/usr/bin/env bash
# The one gate: formatting, lints and tests of every workspace in this checkout, and of the
# lessons checked out next to it. CI runs exactly this script, so a green run here is a green
# pull request — and a red one names the workspace that drifted.
#
#   nix develop -c bash tools/check.sh
#
# Three workspaces live here and cargo fmt/cargo test at the root never reach two of them:
# the engine (this directory), apps/mobile (the library iOS links) and apps/un (the desktop
# product). unmath is a separate repository that needs ../unmath; without it the lesson steps
# are skipped, which is how the mobile CI job runs.
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
engine=$(dirname "$here")
lessons=$engine/../unmath
cd "$engine"

step() { printf '\n== %s\n' "$1"; }

step "rustfmt"
for ws in . apps/mobile apps/un; do
    ( cd "$ws" && cargo fmt --all --check ) || {
        printf '::error::%s is not formatted: run cargo fmt --all there\n' "$ws" >&2
        exit 1
    }
done
if [ -d "$lessons" ]; then
    ( cd "$lessons" && cargo fmt --all --check ) || {
        printf '::error::unmath is not formatted: run cargo fmt --all there\n' >&2
        exit 1
    }
fi

step "clippy"
cargo clippy --workspace --all-targets -- -D warnings
( cd apps/mobile && cargo clippy --all-targets -- -D warnings )
( cd apps/un && cargo clippy --all-targets -- -D warnings )
if [ -d "$lessons" ]; then
    ( cd "$lessons" && cargo clippy --all-targets -- -D warnings )
fi

step "tests"
cargo test --workspace
( cd apps/un && cargo test )
if [ -d "$lessons" ]; then
    ( cd "$lessons" && cargo test )
fi

step "ok: formatting, lints and tests are green"
