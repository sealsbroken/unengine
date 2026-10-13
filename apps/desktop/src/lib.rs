//! The desktop window: camera preview with the table overlay, the tutor HUD and a number pad.
//!
//! Perception runs on its own thread and publishes the latest frame and table state; the
//! UI thread draws at 60 fps and talks to the engine. Products call [`run`] with their own
//! strategy registry.

mod app;
mod overlay;
mod settings;
mod worker;

pub use app::{run, UnApp};
