//! Frame in, table state out: detector, hand landmarks, estimator and tracker in one call.

use std::path::Path;

use thiserror::Error;
use un_core::{
    touched_object, Box2, Detection, DetectionTracker, EstimatorConfig, HandObservation, LayoutFreeze, TableState,
    TableStateEstimator, TrackedDetection, TrackerConfig, TOUCH_MARGIN,
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
/// The detector's hand decoy: never counted, but it seeds the hand landmarks.
pub const HAND_LABEL: &str = "_hand";
/// Hand and finger boxes this sure seed the landmarks: the landmark model then decides whether it
/// is a hand, so a weak box costs one landmark run at most.
pub const SEED_CONFIDENCE: f32 = 0.1;
/// Landmark score a seeded hand must reach. A detector box already says a hand is there, and a
/// hand reaching in from the edge, its palm out of the picture, scores 0.3 where a whole one
/// scores 0.9.
pub const SEEDED_LANDMARK_THRESHOLD: f32 = 0.3;
/// A hand or finger box this sure holds the toys' layout (see `un_core::freeze`): a finger
/// reaching in from the edge is often seen at 0.15 to 0.3 before its landmarks fit.
pub const FREEZE_CONFIDENCE: f32 = 0.15;

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
    /// Confidence an object needs to be counted (the detector itself reports down to the seeds').
    object_confidence: f32,
    /// The detector has hand or finger labels: new hands are found from its boxes, no palm detector.
    seeded: bool,
    pub hands: Option<HandLandmarker>,
    pub estimator: TableStateEstimator,
    pub tracker: DetectionTracker,
    pub count_label: Option<String>,
    pub labels: Vec<String>,
    /// The object a finger was on in the previous frame; it keeps its claim while still touched.
    last_pointed: Option<u32>,
    /// The toys' layout, held while a hand is over the table.
    freeze: LayoutFreeze,
}

impl Perception {
    pub fn load(paths: &ModelPaths, config: &PerceptionConfig) -> Result<Self, PerceptionError> {
        let detector_path = paths.detector(&config.detector);
        if !detector_path.exists() {
            return Err(PerceptionError::NoDetector(detector_path.display().to_string()));
        }
        let mut detector = Detector::load(
            &detector_path,
            DetectorConfig { confidence: config.confidence, provider: config.provider, ..Default::default() },
        )?;
        let seeded = config.hands && detector.labels.iter().any(|l| l == HAND_LABEL || l == FINGER_LABEL);
        if seeded {
            detector.decoder.confidence = config.confidence.min(SEED_CONFIDENCE);
        }
        let landmarks_installed = paths.hand_landmark().exists();
        let hands = if config.hands && landmarks_installed && (seeded || paths.hands_installed()) {
            let palm = paths.palm_detector();
            Some(HandLandmarker::load(
                (!seeded).then_some(palm.as_path()),
                &paths.hand_landmark(),
                // The hand models stay on the CPU: they are small, and CoreML rejects the palm
                // detector's 2016-anchor output ("static output shape and inferred shape have
                // different ranks"), which fails every frame.
                HandLandmarkerConfig {
                    mirrored: config.mirrored,
                    provider: if config.provider == Provider::Auto { Provider::Cpu } else { config.provider },
                    landmark_threshold: if seeded { SEEDED_LANDMARK_THRESHOLD } else { 0.5 },
                    ..Default::default()
                },
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
            object_confidence: config.confidence,
            seeded,
            hands,
            estimator: TableStateEstimator::new(config.estimator.clone()),
            tracker: DetectionTracker::new(TrackerConfig::default()),
            count_label: config.count_label.clone(),
            last_pointed: None,
            freeze: LayoutFreeze::default(),
            labels,
        })
    }

    pub fn detector_path(&self) -> &Path {
        &self.detector.path
    }

    pub fn process(&mut self, frame: &RgbFrame, timestamp: f64) -> Result<PerceptionOutput, PerceptionError> {
        let all = self.detector.detect_all(frame)?;
        let is_seed = |d: &Detection| matches!(d.label.as_deref(), Some(HAND_LABEL) | Some(FINGER_LABEL));
        let mut seeds: Vec<&Detection> = if self.seeded { all.iter().filter(|d| is_seed(d)).collect() } else { vec![] };
        seeds.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));
        let seed_boxes: Vec<Box2> = seeds.iter().map(|d| d.bbox).collect();
        let sure: Vec<Detection> =
            all.iter().filter(|d| !d.is_decoy() && d.confidence >= self.object_confidence).cloned().collect();
        let (fingers, mut detections) = split_fingers(sure);
        if let Some(label) = &self.count_label {
            detections.retain(|d| d.label.as_deref() == Some(label));
        }
        let hands = match &mut self.hands {
            Some(tracker) => tracker.track(frame, &seed_boxes)?,
            None => vec![],
        };
        // Any part of a hand over the table (landmarks, or a hand or finger box) stops toy
        // recognition: the toys stay as they were (see `un_core::freeze`) and nothing is counted.
        let hand_over_table = !hands.is_empty() || seeds.iter().any(|d| d.confidence >= FREEZE_CONFIDENCE);
        if hand_over_table {
            detections = self.freeze.held_toys();
        }
        // A hand seen only as a box keeps the table "busy" too (the count waits for it to leave),
        // but it neither points nor counts fingers.
        let mut present = hands.clone();
        for seed in seeds.iter().filter(|d| d.confidence >= FREEZE_CONFIDENCE) {
            if !hands.iter().any(|h| h.bbox.is_some_and(|b| b.intersection(&seed.bbox).is_some())) {
                present.push(HandObservation {
                    chirality: un_core::Chirality::Unknown,
                    joints: Default::default(),
                    joint_confidences: Default::default(),
                    raised_finger_count: None,
                    bbox: Some(seed.bbox),
                    score: seed.confidence,
                });
            }
        }
        let mut state = self.estimator.ingest(&detections, &present, timestamp);
        let hand_over_table = hand_over_table || state.hands_present;
        let (tracked, numbered) = self.freeze.step(hand_over_table, &mut self.tracker, &state.detections);
        if self.freeze.holding() {
            // What is shown and pointed at is the held layout, not this frame's boxes.
            state.detections = tracked.iter().map(|t| t.detection.clone()).collect();
        }
        state.pointed_at = touched_object(&numbered, &hands, TOUCH_MARGIN, self.last_pointed);
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
        state.pointed_at = touched_object(&tracker.numbered(), hands, TOUCH_MARGIN, previous);
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
        // Once the objects are numbered (two frames): a finger box alone never points, only an
        // index fingertip from the landmarks does.
        let out = Perception::process_detections(&mut estimator, &mut tracker, &detections, &[], 0.1, None);
        assert_eq!(out.state.pointed_at, None);
    }
}
