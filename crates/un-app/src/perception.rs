//! Frame in, table state out: detector, hand landmarks, estimator and tracker in one call.

use std::path::Path;

use thiserror::Error;
use un_core::{
    touched_object, Detection, DetectionTracker, EstimatorConfig, HandObservation, TableState, TableStateEstimator,
    TrackedDetection, TrackerConfig, TOUCH_MARGIN,
};
use un_vision::{Detector, DetectorConfig, HandLandmarker, HandLandmarkerConfig, Provider, RgbFrame, VisionError};

use crate::paths::ModelPaths;

#[derive(Debug, Error)]
pub enum PerceptionError {
    #[error("{0}")]
    Vision(#[from] VisionError),
    #[error("detector not found: {0}. Build one with tools/export_detector.py")]
    NoDetector(String),
}

#[derive(Debug, Clone)]
pub struct PerceptionConfig {
    /// Detector name (`toys`, or one taught with tools/teach.py) or a path to an `.onnx`.
    pub detector: String,
    pub confidence: f32,
    pub provider: Provider,
    pub hands: bool,
    /// Count only this label.
    pub count_label: Option<String>,
    pub estimator: EstimatorConfig,
    pub mirrored: bool,
}

impl Default for PerceptionConfig {
    fn default() -> Self {
        Self {
            detector: "toys".into(),
            confidence: 0.25,
            provider: Provider::Cpu,
            hands: true,
            count_label: None,
            estimator: EstimatorConfig::default(),
            mirrored: false,
        }
    }
}

/// The detector label for the child's finger: shown over the picture, never counted as an
/// object. `tools/export_detector.py` adds it with `--class "finger=finger,index finger,..."`.
pub const FINGER_LABEL: &str = "finger";

pub struct PerceptionOutput {
    pub state: TableState,
    pub tracked: Vec<TrackedDetection>,
    pub hands: Vec<HandObservation>,
    /// Fingers seen in this frame (the `finger` label), kept out of the count.
    pub fingers: Vec<Detection>,
}

/// Splits the finger detections off the objects.
fn split_fingers(detections: Vec<Detection>) -> (Vec<Detection>, Vec<Detection>) {
    detections.into_iter().partition(|d| d.label.as_deref() == Some(FINGER_LABEL))
}

pub struct Perception {
    pub detector: Detector,
    pub hands: Option<HandLandmarker>,
    pub estimator: TableStateEstimator,
    pub tracker: DetectionTracker,
    pub count_label: Option<String>,
    pub labels: Vec<String>,
    /// The object a finger was on in the previous frame; it keeps its claim while still touched.
    last_pointed: Option<u32>,
}

impl Perception {
    pub fn load(paths: &ModelPaths, config: &PerceptionConfig) -> Result<Self, PerceptionError> {
        let detector_path = paths.detector(&config.detector);
        if !detector_path.exists() {
            return Err(PerceptionError::NoDetector(detector_path.display().to_string()));
        }
        let detector = Detector::load(
            &detector_path,
            DetectorConfig { confidence: config.confidence, provider: config.provider, ..Default::default() },
        )?;
        let hands = if config.hands && paths.hands_installed() {
            Some(HandLandmarker::load(
                &paths.palm_detector(),
                &paths.hand_landmark(),
                HandLandmarkerConfig { mirrored: config.mirrored, provider: config.provider, ..Default::default() },
            )?)
        } else {
            if config.hands {
                log::warn!("hand models missing in {}; hand tracking off (run tools/fetch_models.py)", paths.root.display());
            }
            None
        };
        let mut labels = detector.object_labels();
        labels.retain(|l| l != FINGER_LABEL);
        Ok(Self {
            detector,
            hands,
            estimator: TableStateEstimator::new(config.estimator.clone()),
            tracker: DetectionTracker::new(TrackerConfig::default()),
            count_label: config.count_label.clone(),
            last_pointed: None,
            labels,
        })
    }

    pub fn detector_path(&self) -> &Path {
        &self.detector.path
    }

    pub fn process(&mut self, frame: &RgbFrame, timestamp: f64) -> Result<PerceptionOutput, PerceptionError> {
        let (fingers, mut detections) = split_fingers(self.detector.detect(frame)?);
        if let Some(label) = &self.count_label {
            detections.retain(|d| d.label.as_deref() == Some(label));
        }
        let hands = match &mut self.hands {
            Some(tracker) => tracker.track(frame)?,
            None => vec![],
        };
        let mut state = self.estimator.ingest(&detections, &hands, timestamp);
        let tracked = self.tracker.update(&state.detections);
        state.pointed_at = touched_object(&self.tracker.numbered(), &hands, &fingers, TOUCH_MARGIN, self.last_pointed);
        self.last_pointed = state.pointed_at;
        Ok(PerceptionOutput { state, tracked, hands, fingers })
    }

    /// Perception without a camera: the simulated table's detections through the same state
    /// machines; `previous` is the last output's `pointed_at`.
    pub fn process_detections(
        estimator: &mut TableStateEstimator,
        tracker: &mut DetectionTracker,
        detections: &[Detection],
        hands: &[HandObservation],
        timestamp: f64,
        previous: Option<u32>,
    ) -> PerceptionOutput {
        let (fingers, detections) = split_fingers(detections.to_vec());
        let mut state = estimator.ingest(&detections, hands, timestamp);
        let tracked = tracker.update(&state.detections);
        state.pointed_at = touched_object(&tracker.numbered(), hands, &fingers, TOUCH_MARGIN, previous);
        PerceptionOutput { state, tracked, hands: hands.to_vec(), fingers }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use un_core::Box2;

    #[test]
    fn a_finger_is_shown_but_never_counted() {
        let mut estimator = TableStateEstimator::default();
        let mut tracker = DetectionTracker::new(TrackerConfig::default());
        let detections = vec![
            Detection::labeled(Box2::new(0.1, 0.1, 0.2, 0.2), 0.9, "car"),
            Detection::labeled(Box2::new(0.5, 0.5, 0.6, 0.6), 0.9, "car"),
            Detection::labeled(Box2::new(0.55, 0.3, 0.6, 0.55), 0.8, FINGER_LABEL),
        ];
        let out = Perception::process_detections(&mut estimator, &mut tracker, &detections, &[], 0.0, None);
        assert_eq!(out.state.raw_count, 2);
        assert_eq!(out.state.detections.len(), 2);
        assert_eq!(out.fingers.len(), 1);
        assert_eq!(out.fingers[0].label.as_deref(), Some("finger"));
        // Once the objects are numbered (two frames), the finger over the second one is on it.
        let out = Perception::process_detections(&mut estimator, &mut tracker, &detections, &[], 0.1, None);
        assert_eq!(out.state.pointed_at, Some(2));
    }
}
