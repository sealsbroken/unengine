//! Everything an app needs to run a session, minus the window.
//!
//! * [`paths`]: where the models are.
//! * [`speech_setup`]: the microphone and the voice for a language, or doubles.
//! * [`Perception`]: detector + hands + estimator + tracker on a frame.
//! * [`SimulatedTable`]: a table without a camera.
//! * [`Session`]: strategy + engine + perception, with a JSON transcript for headless runs.

pub mod paths;
pub mod perception;
pub mod session;
pub mod sim;

pub use paths::ModelPaths;
pub use perception::{Perception, PerceptionConfig, PerceptionOutput};
pub use session::{build_session, list_microphones, registry, speech_setup, MicChoice, Session, SessionOptions, VoiceChoice};
pub use sim::SimulatedTable;
