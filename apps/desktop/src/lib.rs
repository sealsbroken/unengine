//! The desktop window, as the "Un Desktop Board" designs it: the children, the trail, the lesson
//! over the webcam picture and its end, a Settings panel for the grown-up.
//!
//! Perception runs on its own thread and publishes the latest frame and table state; the
//! UI thread draws at 60 fps and talks to the engine. Products call [`run`] with their own
//! strategy registry.

mod app;
mod display;
mod overlay;
mod settings;
mod un;
mod worker;

pub use app::{run, run_with, Defaults, UnApp};
