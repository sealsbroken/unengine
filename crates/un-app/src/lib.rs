//! Everything an app needs to run a session, minus the window.
//!
//! * [`paths`]: where the models are.
//! * [`speech_setup`]: the microphone and the voice for a language, or doubles.
//! * [`Perception`]: detector + estimator + tracker on a frame.
//! * [`SimulatedTable`]: a table without a camera.
//! * [`Profiles`]: the children and their progress through the lessons.
//! * [`Session`]: strategy + engine + perception, with a JSON transcript for headless runs.

pub mod bundle;
pub mod paths;
pub mod perception;
pub mod profiles;
pub mod session;
pub mod sim;

pub use paths::ModelPaths;
pub use perception::{Perception, PerceptionConfig, PerceptionOutput};
pub use profiles::{Profile, Profiles};
#[cfg(feature = "speech")]
pub use session::list_microphones;
pub use session::{
    build_session, pick_microphone, registry, set_registry, speech_setup, MicChoice, Session, SessionOptions, VoiceChoice,
};
pub use sim::SimulatedTable;
