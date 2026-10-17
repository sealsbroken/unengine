//! What an object detector reports for one frame.

use serde::{Deserialize, Serialize};

use crate::geometry::Box2;

/// One detected object. `label` is `None` for single-class detectors.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Detection {
    pub bbox: Box2,
    pub confidence: f32,
    pub label: Option<String>,
}

impl Detection {
    pub fn new(bbox: Box2, confidence: f32) -> Self {
        Self { bbox, confidence, label: None }
    }

    pub fn labeled(bbox: Box2, confidence: f32, label: impl Into<String>) -> Self {
        Self { bbox, confidence, label: Some(label.into()) }
    }

    /// Labels starting with `_` are decoys: things the detector is taught to recognise only so
    /// that they are *not* mistaken for objects (the keyboard, a hand, a finger). They are never counted.
    pub fn is_decoy(&self) -> bool {
        self.label.as_deref().is_some_and(|l| l.starts_with('_'))
    }

    pub fn label_or(&self, default: &str) -> String {
        self.label.clone().unwrap_or_else(|| default.to_string())
    }
}
