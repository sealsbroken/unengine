//! Platform-independent core of Un.
//!
//! Nothing here touches a camera, a microphone or a screen. The crate holds the pure
//! state machines that every platform shares:
//!
//! * [`geometry`]: normalized boxes and points (origin top-left, both axes 0...1).
//! * [`detection`]: what an object detector reports.
//! * [`hands`]: hand joints, finger counting and hand observations.
//! * [`estimator`]: the debounced table state (committed count, stability, zones).
//! * [`tracker`]: stable 1...n numbering of the objects on the table.
//! * [`language`]: languages, number words and plural agreement.
//! * [`numbers`]: forgiving number parsing and "how many ...?" questions.
//! * [`strategy`]: the contract between the engine and a lesson strategy (the methodics).
//!
//! The lesson content itself (tasks, difficulty ladders, what the tutor says) is a
//! *strategy* implemented outside this crate; see [`strategy::Strategy`].

pub mod detection;
pub mod estimator;
pub mod freeze;
pub mod geometry;
pub mod hands;
pub mod language;
pub mod numbers;
pub mod pointing;
pub mod strategy;
pub mod tracker;

pub use detection::Detection;
pub use estimator::{EstimatorConfig, TableState, TableStateEstimator};
pub use freeze::LayoutFreeze;
pub use geometry::{Box2, Point};
pub use hands::{Chirality, FingerCounter, HandJoint, HandObservation};
pub use language::{Language, Noun, Numerals};
pub use numbers::{CountQuestion, NumberParser, NumberWords};
pub use pointing::{touched_object, TOUCH_MARGIN};
pub use strategy::{
    Effect, Event, ListenMode, SessionConfig, SessionSummary, Strategy, StrategyFactory, StrategyInfo, TableObservation,
    TaskRecord,
};
pub use tracker::{DetectionTracker, TrackedDetection, TrackerConfig};
