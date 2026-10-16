# Un — notes for Claude Code

- Rust workspace; `cargo test --workspace` must stay green on macOS, Linux and Windows (there is no CI: run it before pushing).
- Everything is on-device: no network calls at runtime, no analytics. Models are fetched by `tools/fetch_models.py`.
- `un-core` is pure and platform-independent; perception and speech I/O never leak into it.
- Lesson content (tasks, difficulty, wording) is a *strategy* behind `un_core::strategy::Strategy`. The only strategy in
  this repository is `strategies/demo`; the real methodics live in the private `unmethod` repository, checked out as a
  sibling directory (`../unmethod`) and built from there.
- This repository is LGPL-3.0-or-later. Proprietary strategies plug in through the public `Strategy` trait and link-time
  registration (`inventory`); keep that boundary clean so the engine stays reusable.
- Formatting: `cargo fmt` with `rustfmt.toml` (130 columns). Lints: `cargo clippy --all-targets -- -D warnings`.
- The desktop GUI is `apps/desktop` (egui); headless flows go through `crates/un-cli` (`un lesson --sim ...`).
- Mobile shells (`apps/ios`, `apps/android`) consume `crates/un-ffi` through UniFFI; regenerate bindings with
  `cargo run -p un-ffi --bin uniffi-bindgen -- generate --library <lib> --language swift|kotlin`.
